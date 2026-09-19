// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import "@openzeppelin/contracts/utils/cryptography/EIP712.sol";
import "@openzeppelin/contracts/utils/cryptography/SignatureChecker.sol";
import "@openzeppelin/contracts/access/Ownable2Step.sol";

interface IL2SequencerUptimeFeed {
    function latestRoundData()
        external
        view
        returns (uint80 roundId, int256 answer, uint256 startedAt, uint256 updatedAt, uint80 answeredInRound);
}

interface IVelaCustodyVault {
    function isVelaCustodyVault() external pure returns (bool);
    function supportedAssets(address asset) external view returns (bool);
}

/// @title VelaSettlement
/// @notice Immutable omnibus custody vault. Customer beneficial ownership is
/// authoritative only in the off-chain ledger; this contract deliberately has
/// no per-customer balances and no unsigned customer withdrawal path.
contract VelaSettlement is ReentrancyGuard, Ownable2Step, EIP712, IVelaCustodyVault {
    using SafeERC20 for IERC20;

    bytes32 public constant WITHDRAWAL_TYPEHASH = keccak256(
        "Withdrawal(bytes16 requestId,address destination,address asset,uint256 amount,uint256 nonce,uint256 deadline,bytes32 policyHash,bytes32 approverHash)"
    );
    bytes32 public constant MIGRATION_TYPEHASH = keccak256(
        "Migration(uint256 proposalId,address successor,address asset,uint256 amount,uint256 deadline,bytes32 liabilitySnapshotHash)"
    );

    address public constant ETH = address(0);
    uint256 public constant OPERATOR_ROTATION_DELAY = 48 hours;
    uint256 public constant MIGRATION_DELAY = 7 days;
    uint256 public constant SEQUENCER_GRACE_PERIOD = 1 hours;

    struct Withdrawal {
        bytes16 requestId;
        address destination;
        address asset;
        uint256 amount;
        uint256 nonce;
        uint256 deadline;
        bytes32 policyHash;
        bytes32 approverHash;
    }

    struct MigrationAuthorization {
        uint256 proposalId;
        address successor;
        address asset;
        uint256 amount;
        uint256 deadline;
        bytes32 liabilitySnapshotHash;
    }

    address public operator;
    address public guardian;
    bytes32 public immutable sourceCommit;
    address public pendingOperator;
    uint256 public pendingOperatorEta;

    bool public depositsPaused;
    bool public withdrawalsPaused;

    mapping(address => bool) public supportedAssets;
    mapping(address => bool) public registeredAssets;
    mapping(address => bool) public migratedAssets;
    mapping(bytes16 => bool) public usedWithdrawalRequestIds;
    mapping(uint256 => bool) public usedWithdrawNonces;
    uint256 public depositNonce;

    mapping(address => uint256) public totalDeposited;
    mapping(address => uint256) public totalWithdrawn;
    mapping(address => uint256) public totalMigrated;

    IL2SequencerUptimeFeed public sequencerUptimeFeed;

    uint256 public migrationProposalId;
    address public proposedSuccessorVault;
    bytes32 public proposedLiabilitySnapshotHash;
    uint256 public proposedSuccessorEta;
    bool public successorApprovedByGuardian;

    mapping(uint256 => bytes32) public anchoredStateRoots;
    uint256 public anchorCount;

    error ZeroAmount();
    error ZeroAddress();
    error ZeroAccountId();
    error ZeroSourceCommit();
    error InvalidEvidenceHash();
    error UseDepositETHForNative();
    error AssetNotSupported();
    error AssetAlreadyMigrated();
    error UnsupportedTransferBehavior();
    error InsufficientVaultBalance();
    error InvalidSignature();
    error NonceAlreadyUsed();
    error WithdrawalRequestAlreadyUsed();
    error InvalidWithdrawalRequestId();
    error WithdrawalExpired();
    error EthTransferFailed();
    error DepositsArePaused();
    error WithdrawalsArePaused();
    error NotOperator();
    error NotGuardian();
    error NotGuardianOrOwner();
    error RoleCollision();
    error NoPendingOperator();
    error OperatorTimelockActive();
    error SequencerDown();
    error SequencerGraceActive();
    error MigrationRequiresFullPause();
    error InvalidSuccessorVault();
    error NoMigrationProposal();
    error MigrationNotGuardianApproved();
    error MigrationTimelockActive();
    error MigrationAuthorizationMismatch();
    error MigrationExpired();
    error OwnershipRenunciationDisabled();

    event Deposited(
        bytes32 indexed depositId, address indexed depositor, bytes32 indexed accountId, address asset, uint256 amount
    );
    event WithdrawalExecuted(
        bytes16 indexed requestId,
        address indexed destination,
        address indexed asset,
        uint256 amount,
        uint256 nonce,
        uint256 deadline,
        bytes32 policyHash,
        bytes32 approverHash
    );
    event DepositPauseUpdated(bool paused, address indexed actor);
    event WithdrawalPauseUpdated(bool paused, address indexed actor);
    event OperatorProposed(address indexed proposed, uint256 eta);
    event OperatorRotated(address indexed previousOperator, address indexed newOperator);
    event GuardianRotated(address indexed previousGuardian, address indexed newGuardian);
    event SequencerFeedUpdated(address indexed feed);
    event SupportedAssetUpdated(address indexed asset, bool supported);
    event DeploymentIdentity(uint256 indexed chainId, bytes32 indexed sourceCommit);
    event SuccessorVaultProposed(
        uint256 indexed proposalId, address indexed successor, bytes32 indexed liabilitySnapshotHash, uint256 eta
    );
    event SuccessorVaultGuardianApproved(uint256 indexed proposalId, address indexed guardian);
    event SuccessorVaultProposalCancelled(uint256 indexed proposalId, address indexed actor);
    event AssetMigrated(
        uint256 indexed proposalId,
        address indexed successor,
        address indexed asset,
        uint256 amount,
        bytes32 liabilitySnapshotHash
    );
    event DirectETHReceived(address indexed sender, uint256 amount);
    event StateRootAnchored(uint256 indexed anchorId, bytes32 stateRoot, uint256 timestamp, uint256 ordersProcessed);

    constructor(address _operator, bytes32 _sourceCommit) Ownable(msg.sender) EIP712("VelaSettlement", "3") {
        if (_operator == address(0)) revert ZeroAddress();
        if (_operator == msg.sender) revert RoleCollision();
        if (_sourceCommit == bytes32(0)) revert ZeroSourceCommit();
        operator = _operator;
        guardian = msg.sender;
        sourceCommit = _sourceCommit;
        supportedAssets[ETH] = true;
        registeredAssets[ETH] = true;
        emit SupportedAssetUpdated(ETH, true);
        emit DeploymentIdentity(block.chainid, _sourceCommit);
    }

    modifier onlyOperator() {
        if (msg.sender != operator) revert NotOperator();
        _;
    }

    modifier onlyGuardian() {
        if (msg.sender != guardian) revert NotGuardian();
        _;
    }

    modifier onlyGuardianOrOwner() {
        if (msg.sender != guardian && msg.sender != owner()) revert NotGuardianOrOwner();
        _;
    }

    modifier whenDepositsActive() {
        if (depositsPaused) revert DepositsArePaused();
        _;
    }

    modifier whenWithdrawalsActive() {
        if (withdrawalsPaused) revert WithdrawalsArePaused();
        _;
    }

    modifier sequencerUp() {
        _checkSequencer();
        _;
    }

    function isVelaCustodyVault() external pure returns (bool) {
        return true;
    }

    // Omnibus deposits bind custody evidence to an opaque ledger account ID.
    function depositETH(bytes32 accountId) external payable nonReentrant whenDepositsActive {
        if (accountId == bytes32(0)) revert ZeroAccountId();
        if (msg.value == 0) revert ZeroAmount();
        if (migratedAssets[ETH]) revert AssetAlreadyMigrated();
        totalDeposited[ETH] += msg.value;
        emit Deposited(_nextDepositId(msg.sender, accountId, ETH), msg.sender, accountId, ETH, msg.value);
    }

    function depositToken(bytes32 accountId, address asset, uint256 amount) external nonReentrant whenDepositsActive {
        if (accountId == bytes32(0)) revert ZeroAccountId();
        if (amount == 0) revert ZeroAmount();
        if (asset == ETH) revert UseDepositETHForNative();
        if (!supportedAssets[asset]) revert AssetNotSupported();
        if (migratedAssets[asset]) revert AssetAlreadyMigrated();
        uint256 beforeBalance = IERC20(asset).balanceOf(address(this));
        IERC20(asset).safeTransferFrom(msg.sender, address(this), amount);
        uint256 received = IERC20(asset).balanceOf(address(this)) - beforeBalance;
        if (received != amount) revert UnsupportedTransferBehavior();
        totalDeposited[asset] += received;
        emit Deposited(_nextDepositId(msg.sender, accountId, asset), msg.sender, accountId, asset, received);
    }

    function _nextDepositId(address depositor, bytes32 accountId, address asset) private returns (bytes32) {
        uint256 nonce = depositNonce++;
        return keccak256(abi.encode(block.chainid, address(this), depositor, accountId, asset, nonce));
    }

    function withdrawalDigest(Withdrawal calldata withdrawal) public view returns (bytes32) {
        return _hashTypedDataV4(
            keccak256(
                abi.encode(
                    WITHDRAWAL_TYPEHASH,
                    withdrawal.requestId,
                    withdrawal.destination,
                    withdrawal.asset,
                    withdrawal.amount,
                    withdrawal.nonce,
                    withdrawal.deadline,
                    withdrawal.policyHash,
                    withdrawal.approverHash
                )
            )
        );
    }

    /// @notice Anyone may relay an authorization, but every value-moving and
    /// policy field is signed by the withdrawal authorizer.
    function executeWithdrawal(Withdrawal calldata withdrawal, bytes calldata signature)
        external
        nonReentrant
        whenWithdrawalsActive
        sequencerUp
    {
        if (withdrawal.amount == 0) revert ZeroAmount();
        if (!registeredAssets[withdrawal.asset]) revert AssetNotSupported();
        if (migratedAssets[withdrawal.asset]) revert AssetAlreadyMigrated();
        if (withdrawal.requestId == bytes16(0)) revert InvalidWithdrawalRequestId();
        if (withdrawal.destination == address(0) || withdrawal.destination == address(this)) revert ZeroAddress();
        if (withdrawal.policyHash == bytes32(0) || withdrawal.approverHash == bytes32(0)) {
            revert InvalidEvidenceHash();
        }
        if (block.timestamp > withdrawal.deadline) revert WithdrawalExpired();
        if (usedWithdrawalRequestIds[withdrawal.requestId]) revert WithdrawalRequestAlreadyUsed();
        if (usedWithdrawNonces[withdrawal.nonce]) revert NonceAlreadyUsed();
        if (vaultBalance(withdrawal.asset) < withdrawal.amount) revert InsufficientVaultBalance();
        if (!_isOperatorSignature(withdrawalDigest(withdrawal), signature)) revert InvalidSignature();

        usedWithdrawalRequestIds[withdrawal.requestId] = true;
        usedWithdrawNonces[withdrawal.nonce] = true;
        totalWithdrawn[withdrawal.asset] += withdrawal.amount;
        _transferAsset(withdrawal.asset, withdrawal.destination, withdrawal.amount);
        emit WithdrawalExecuted(
            withdrawal.requestId,
            withdrawal.destination,
            withdrawal.asset,
            withdrawal.amount,
            withdrawal.nonce,
            withdrawal.deadline,
            withdrawal.policyHash,
            withdrawal.approverHash
        );
    }

    function _isOperatorSignature(bytes32 digest, bytes calldata signature) private view returns (bool) {
        if (operator.code.length != 0) {
            return SignatureChecker.isValidSignatureNowCalldata(operator, digest, signature);
        }
        if (signature.length == 65) {
            bytes memory normalized = signature;
            uint8 v = uint8(normalized[64]);
            if (v < 27) normalized[64] = bytes1(v + 27);
            return SignatureChecker.isValidSignatureNow(operator, digest, normalized);
        }
        return SignatureChecker.isValidSignatureNowCalldata(operator, digest, signature);
    }

    // Deposits and withdrawals can be halted independently. Guardian can only
    // halt; owner approval is required to resume.
    function paused() external view returns (bool) {
        return depositsPaused && withdrawalsPaused;
    }

    function pause() external onlyGuardianOrOwner {
        _setDepositPause(true);
        _setWithdrawalPause(true);
    }

    function unpause() external onlyOwner {
        _setDepositPause(false);
        _setWithdrawalPause(false);
    }

    function pauseDeposits() external onlyGuardianOrOwner {
        _setDepositPause(true);
    }

    function pauseWithdrawals() external onlyGuardianOrOwner {
        _setWithdrawalPause(true);
    }

    function unpauseDeposits() external onlyOwner {
        _setDepositPause(false);
    }

    function unpauseWithdrawals() external onlyOwner {
        _setWithdrawalPause(false);
    }

    function _setDepositPause(bool value) private {
        if (depositsPaused == value) return;
        depositsPaused = value;
        emit DepositPauseUpdated(value, msg.sender);
    }

    function _setWithdrawalPause(bool value) private {
        if (withdrawalsPaused == value) return;
        withdrawalsPaused = value;
        emit WithdrawalPauseUpdated(value, msg.sender);
    }

    // Migration requires five controls: full pause, owner proposal/execution,
    // independent guardian approval, delay, and exact operator authorization.
    function proposeSuccessorVault(address successor, bytes32 liabilitySnapshotHash) external onlyOwner {
        if (!depositsPaused || !withdrawalsPaused) revert MigrationRequiresFullPause();
        if (!_isValidSuccessor(successor)) revert InvalidSuccessorVault();
        if (liabilitySnapshotHash == bytes32(0)) revert InvalidEvidenceHash();
        uint256 proposalId = ++migrationProposalId;
        proposedSuccessorVault = successor;
        proposedLiabilitySnapshotHash = liabilitySnapshotHash;
        proposedSuccessorEta = block.timestamp + MIGRATION_DELAY;
        successorApprovedByGuardian = false;
        emit SuccessorVaultProposed(proposalId, successor, liabilitySnapshotHash, proposedSuccessorEta);
    }

    function approveSuccessorVault(uint256 proposalId) external onlyGuardian {
        if (proposalId == 0 || proposalId != migrationProposalId || proposedSuccessorVault == address(0)) {
            revert NoMigrationProposal();
        }
        if (guardian == owner() || guardian == operator) revert RoleCollision();
        successorApprovedByGuardian = true;
        emit SuccessorVaultGuardianApproved(proposalId, msg.sender);
    }

    function cancelSuccessorVault() external onlyGuardianOrOwner {
        uint256 proposalId = migrationProposalId;
        if (proposalId == 0 || proposedSuccessorVault == address(0)) revert NoMigrationProposal();
        proposedSuccessorVault = address(0);
        proposedLiabilitySnapshotHash = bytes32(0);
        proposedSuccessorEta = 0;
        successorApprovedByGuardian = false;
        emit SuccessorVaultProposalCancelled(proposalId, msg.sender);
    }

    function migrationDigest(MigrationAuthorization calldata authorization) public view returns (bytes32) {
        return _hashTypedDataV4(
            keccak256(
                abi.encode(
                    MIGRATION_TYPEHASH,
                    authorization.proposalId,
                    authorization.successor,
                    authorization.asset,
                    authorization.amount,
                    authorization.deadline,
                    authorization.liabilitySnapshotHash
                )
            )
        );
    }

    function executeMigration(MigrationAuthorization calldata authorization, bytes calldata operatorSignature)
        external
        onlyOwner
        nonReentrant
        sequencerUp
    {
        if (!depositsPaused || !withdrawalsPaused) revert MigrationRequiresFullPause();
        if (authorization.proposalId != migrationProposalId || proposedSuccessorVault == address(0)) {
            revert NoMigrationProposal();
        }
        if (!successorApprovedByGuardian) revert MigrationNotGuardianApproved();
        if (block.timestamp < proposedSuccessorEta) revert MigrationTimelockActive();
        if (block.timestamp > authorization.deadline) revert MigrationExpired();
        if (
            authorization.successor != proposedSuccessorVault
                || authorization.liabilitySnapshotHash != proposedLiabilitySnapshotHash
        ) revert MigrationAuthorizationMismatch();
        if (!registeredAssets[authorization.asset]) revert AssetNotSupported();
        if (migratedAssets[authorization.asset]) revert AssetAlreadyMigrated();
        if (!IVelaCustodyVault(authorization.successor).supportedAssets(authorization.asset)) {
            revert AssetNotSupported();
        }

        uint256 amount = vaultBalance(authorization.asset);
        if (amount == 0) revert ZeroAmount();
        if (authorization.amount != amount) revert MigrationAuthorizationMismatch();
        if (!_isOperatorSignature(migrationDigest(authorization), operatorSignature)) revert InvalidSignature();

        migratedAssets[authorization.asset] = true;
        supportedAssets[authorization.asset] = false;
        totalMigrated[authorization.asset] += amount;
        uint256 beforeBalance = _assetBalance(authorization.asset, authorization.successor);
        _transferAsset(authorization.asset, authorization.successor, amount);
        uint256 received = _assetBalance(authorization.asset, authorization.successor) - beforeBalance;
        if (received != amount) revert UnsupportedTransferBehavior();
        emit SupportedAssetUpdated(authorization.asset, false);
        emit AssetMigrated(
            authorization.proposalId,
            authorization.successor,
            authorization.asset,
            amount,
            authorization.liabilitySnapshotHash
        );
    }

    function _isValidSuccessor(address successor) private view returns (bool) {
        if (successor == address(0) || successor == address(this) || successor.code.length == 0) return false;
        try IVelaCustodyVault(successor).isVelaCustodyVault() returns (bool valid) {
            return valid;
        } catch {
            return false;
        }
    }

    function proposeOperator(address newOperator) external onlyOwner {
        if (newOperator == address(0)) revert ZeroAddress();
        if (newOperator == owner() || newOperator == guardian) revert RoleCollision();
        pendingOperator = newOperator;
        pendingOperatorEta = block.timestamp + OPERATOR_ROTATION_DELAY;
        emit OperatorProposed(newOperator, pendingOperatorEta);
    }

    function acceptProposedOperator() external onlyOwner {
        if (pendingOperator == address(0)) revert NoPendingOperator();
        if (block.timestamp < pendingOperatorEta) revert OperatorTimelockActive();
        address previous = operator;
        operator = pendingOperator;
        pendingOperator = address(0);
        pendingOperatorEta = 0;
        emit OperatorRotated(previous, operator);
    }

    function setGuardian(address newGuardian) external onlyOwner {
        if (newGuardian == address(0)) revert ZeroAddress();
        if (newGuardian == owner() || newGuardian == operator) revert RoleCollision();
        address previous = guardian;
        guardian = newGuardian;
        emit GuardianRotated(previous, newGuardian);
    }

    function transferOwnership(address newOwner) public override onlyOwner {
        if (newOwner == operator || newOwner == guardian) revert RoleCollision();
        super.transferOwnership(newOwner);
    }

    function acceptOwnership() public override {
        if (msg.sender == operator || msg.sender == guardian) revert RoleCollision();
        super.acceptOwnership();
    }

    function renounceOwnership() public pure override {
        revert OwnershipRenunciationDisabled();
    }

    /// @notice Disabling blocks new deposits but not already-authorized exits.
    /// `registeredAssets` is monotonic so delisting cannot trap custody.
    function setSupportedAsset(address asset, bool supported) external onlyOwner {
        if (asset == ETH || asset.code.length == 0) revert AssetNotSupported();
        if (migratedAssets[asset]) revert AssetAlreadyMigrated();
        supportedAssets[asset] = supported;
        if (supported) registeredAssets[asset] = true;
        emit SupportedAssetUpdated(asset, supported);
    }

    function setSequencerFeed(address feed) external onlyOwner {
        sequencerUptimeFeed = IL2SequencerUptimeFeed(feed);
        emit SequencerFeedUpdated(feed);
    }

    function anchorStateRoot(bytes32 stateRoot, uint256 ordersProcessed) external onlyOperator sequencerUp {
        uint256 anchorId = anchorCount++;
        anchoredStateRoots[anchorId] = stateRoot;
        emit StateRootAnchored(anchorId, stateRoot, block.timestamp, ordersProcessed);
    }

    function latestAnchoredStateRoot()
        external
        view
        returns (uint256 anchorId, bytes32 stateRoot, uint256 anchorCount_)
    {
        if (anchorCount == 0) {
            return (0, bytes32(0), 0);
        }
        anchorId = anchorCount - 1;
        stateRoot = anchoredStateRoots[anchorId];
        anchorCount_ = anchorCount;
    }

    function vaultBalance(address asset) public view returns (uint256) {
        return _assetBalance(asset, address(this));
    }

    function _assetBalance(address asset, address account) private view returns (uint256) {
        return asset == ETH ? account.balance : IERC20(asset).balanceOf(account);
    }

    function _transferAsset(address asset, address destination, uint256 amount) private {
        if (asset == ETH) {
            (bool ok,) = destination.call{ value: amount }("");
            if (!ok) revert EthTransferFailed();
        } else {
            IERC20(asset).safeTransfer(destination, amount);
        }
    }

    function _checkSequencer() internal view {
        IL2SequencerUptimeFeed feed = sequencerUptimeFeed;
        if (address(feed) == address(0)) return;
        (, int256 answer,, uint256 updatedAt,) = feed.latestRoundData();
        if (answer != 0) revert SequencerDown();
        if (block.timestamp - updatedAt < SEQUENCER_GRACE_PERIOD) revert SequencerGraceActive();
    }

    receive() external payable {
        emit DirectETHReceived(msg.sender, msg.value);
    }
}
