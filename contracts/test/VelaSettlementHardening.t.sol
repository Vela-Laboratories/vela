// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import "forge-std/Test.sol";
import "../src/VelaSettlement.sol";
import "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import "@openzeppelin/contracts/access/Ownable.sol";

contract HardeningERC20 is ERC20 {
    constructor() ERC20("Mock", "MCK") {
        _mint(msg.sender, 1_000_000e18);
    }
}

contract FeeOnTransferToken is ERC20 {
    constructor() ERC20("Fee", "FEE") {
        _mint(msg.sender, 1_000_000e18);
    }

    function _update(address from, address to, uint256 amount) internal override {
        if (from == address(0) || to == address(0)) return super._update(from, to, amount);
        uint256 fee = amount / 100;
        super._update(from, address(0xdead), fee);
        super._update(from, to, amount - fee);
    }
}

contract VelaSettlementHardeningTest is Test {
    VelaSettlement settlement;
    VelaSettlement successor;
    HardeningERC20 token;
    FeeOnTransferToken feeToken;

    uint256 constant OPERATOR_KEY = 0xA11CE;
    address operator = vm.addr(OPERATOR_KEY);
    address guardian = address(0xCAFE);
    address user = address(0xBEEF);
    bytes32 accountId = keccak256("account");
    bytes32 snapshotHash = keccak256("reconciled-liabilities-at-block-100");

    function setUp() public {
        settlement = new VelaSettlement(operator, keccak256("source"));
        successor = new VelaSettlement(operator, keccak256("successor-source"));
        settlement.setGuardian(guardian);
        successor.setGuardian(address(0xCAFF));
        token = new HardeningERC20();
        feeToken = new FeeOnTransferToken();
        settlement.setSupportedAsset(address(token), true);
        settlement.setSupportedAsset(address(feeToken), true);
        successor.setSupportedAsset(address(token), true);
        vm.deal(user, 10 ether);
        token.transfer(user, 1000e18);
        feeToken.transfer(user, 1000e18);
    }

    function _withdrawal(address asset, uint256 amount, uint256 nonce)
        internal
        view
        returns (VelaSettlement.Withdrawal memory w)
    {
        w = VelaSettlement.Withdrawal({
            requestId: bytes16(keccak256(abi.encode(nonce))),
            destination: user,
            asset: asset,
            amount: amount,
            nonce: nonce,
            deadline: block.timestamp + 30 days,
            policyHash: keccak256("policy"),
            approverHash: keccak256("approval")
        });
    }

    function _withdrawalSignature(VelaSettlement.Withdrawal memory w) internal view returns (bytes memory) {
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(OPERATOR_KEY, settlement.withdrawalDigest(w));
        return abi.encodePacked(r, s, v);
    }

    function _proposeAndApproveMigration() internal {
        settlement.pause();
        settlement.proposeSuccessorVault(address(successor), snapshotHash);
        uint256 proposalId = settlement.migrationProposalId();
        vm.prank(guardian);
        settlement.approveSuccessorVault(proposalId);
    }

    function _migration(address asset, uint256 amount)
        internal
        view
        returns (VelaSettlement.MigrationAuthorization memory authorization)
    {
        authorization = VelaSettlement.MigrationAuthorization({
            proposalId: settlement.migrationProposalId(),
            successor: address(successor),
            asset: asset,
            amount: amount,
            deadline: block.timestamp + 30 days,
            liabilitySnapshotHash: snapshotHash
        });
    }

    function _migrationSignature(VelaSettlement.MigrationAuthorization memory authorization)
        internal
        view
        returns (bytes memory)
    {
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(OPERATOR_KEY, settlement.migrationDigest(authorization));
        return abi.encodePacked(r, s, v);
    }

    function test_independentPauseAndOwnerOnlyResume() public {
        vm.prank(guardian);
        settlement.pauseDeposits();
        assertTrue(settlement.depositsPaused());
        assertFalse(settlement.withdrawalsPaused());

        vm.prank(user);
        vm.expectRevert(VelaSettlement.DepositsArePaused.selector);
        settlement.depositETH{ value: 1 ether }(accountId);

        vm.prank(guardian);
        vm.expectRevert(abi.encodeWithSelector(Ownable.OwnableUnauthorizedAccount.selector, guardian));
        settlement.unpauseDeposits();
        settlement.unpauseDeposits();
        assertFalse(settlement.depositsPaused());
    }

    function test_withdrawalPauseDoesNotStopDeposits() public {
        vm.prank(guardian);
        settlement.pauseWithdrawals();
        vm.prank(user);
        settlement.depositETH{ value: 1 ether }(accountId);
        VelaSettlement.Withdrawal memory w = _withdrawal(address(0), 0.5 ether, 1);
        bytes memory signature = _withdrawalSignature(w);
        vm.expectRevert(VelaSettlement.WithdrawalsArePaused.selector);
        settlement.executeWithdrawal(w, signature);
    }

    function test_disabledAssetBlocksDepositButPermitsLedgerAuthorizedWithdrawal() public {
        vm.startPrank(user);
        token.approve(address(settlement), 10e18);
        settlement.depositToken(accountId, address(token), 10e18);
        vm.stopPrank();
        settlement.setSupportedAsset(address(token), false);

        VelaSettlement.Withdrawal memory w = _withdrawal(address(token), 10e18, 2);
        settlement.executeWithdrawal(w, _withdrawalSignature(w));
        assertEq(token.balanceOf(user), 1000e18);
    }

    function test_feeOnTransferDepositRevertsAtomically() public {
        vm.startPrank(user);
        feeToken.approve(address(settlement), 100e18);
        vm.expectRevert(VelaSettlement.UnsupportedTransferBehavior.selector);
        settlement.depositToken(accountId, address(feeToken), 100e18);
        vm.stopPrank();
        assertEq(feeToken.balanceOf(address(settlement)), 0);
        assertEq(settlement.totalDeposited(address(feeToken)), 0);
    }

    function test_migrationRequiresFullPauseValidVaultAndReconciliationHash() public {
        vm.expectRevert(VelaSettlement.MigrationRequiresFullPause.selector);
        settlement.proposeSuccessorVault(address(successor), snapshotHash);
        settlement.pause();
        vm.expectRevert(VelaSettlement.InvalidSuccessorVault.selector);
        settlement.proposeSuccessorVault(address(0x1234), snapshotHash);
        vm.expectRevert(VelaSettlement.InvalidEvidenceHash.selector);
        settlement.proposeSuccessorVault(address(successor), bytes32(0));
    }

    function test_guardianCannotMoveAssetsOrApproveWhenRolesCollide() public {
        VelaSettlement colliding = new VelaSettlement(operator, keccak256("colliding"));
        colliding.pause();
        colliding.proposeSuccessorVault(address(successor), snapshotHash);
        uint256 collidingProposalId = colliding.migrationProposalId();
        vm.expectRevert(VelaSettlement.RoleCollision.selector);
        colliding.approveSuccessorVault(collidingProposalId);

        _proposeAndApproveMigration();
        vm.warp(block.timestamp + settlement.MIGRATION_DELAY());
        VelaSettlement.MigrationAuthorization memory authorization = _migration(address(0), 1 ether);
        bytes memory authorizationSignature = _migrationSignature(authorization);
        vm.deal(address(settlement), 1 ether);
        vm.prank(guardian);
        vm.expectRevert(abi.encodeWithSelector(Ownable.OwnableUnauthorizedAccount.selector, guardian));
        settlement.executeMigration(authorization, authorizationSignature);
    }

    function test_migrationRequiresGuardianApprovalTimelockAndOperatorSignature() public {
        vm.deal(address(settlement), 1 ether);
        settlement.pause();
        settlement.proposeSuccessorVault(address(successor), snapshotHash);
        VelaSettlement.MigrationAuthorization memory authorization = _migration(address(0), 1 ether);
        bytes memory authorizationSignature = _migrationSignature(authorization);

        vm.expectRevert(VelaSettlement.MigrationNotGuardianApproved.selector);
        settlement.executeMigration(authorization, authorizationSignature);
        vm.prank(guardian);
        settlement.approveSuccessorVault(authorization.proposalId);
        vm.expectRevert(VelaSettlement.MigrationTimelockActive.selector);
        settlement.executeMigration(authorization, authorizationSignature);

        vm.warp(block.timestamp + settlement.MIGRATION_DELAY());
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(0xBAD, settlement.migrationDigest(authorization));
        vm.expectRevert(VelaSettlement.InvalidSignature.selector);
        settlement.executeMigration(authorization, abi.encodePacked(r, s, v));
    }

    function test_migrationMovesOnlyExactFullBalanceAndPermanentlyRetiresAsset() public {
        vm.startPrank(user);
        token.approve(address(settlement), 25e18);
        settlement.depositToken(accountId, address(token), 25e18);
        vm.stopPrank();
        _proposeAndApproveMigration();
        vm.warp(block.timestamp + settlement.MIGRATION_DELAY());

        VelaSettlement.MigrationAuthorization memory wrong = _migration(address(token), 24e18);
        bytes memory wrongSignature = _migrationSignature(wrong);
        vm.expectRevert(VelaSettlement.MigrationAuthorizationMismatch.selector);
        settlement.executeMigration(wrong, wrongSignature);

        VelaSettlement.MigrationAuthorization memory exact = _migration(address(token), 25e18);
        bytes memory exactSignature = _migrationSignature(exact);
        settlement.executeMigration(exact, exactSignature);
        assertEq(token.balanceOf(address(settlement)), 0);
        assertEq(token.balanceOf(address(successor)), 25e18);
        assertEq(settlement.totalMigrated(address(token)), 25e18);
        assertTrue(settlement.migratedAssets(address(token)));

        vm.expectRevert(VelaSettlement.AssetAlreadyMigrated.selector);
        settlement.executeMigration(exact, exactSignature);
        vm.expectRevert(VelaSettlement.AssetAlreadyMigrated.selector);
        settlement.setSupportedAsset(address(token), true);
    }

    function test_guardianCancellationInvalidatesMigration() public {
        vm.deal(address(settlement), 1 ether);
        _proposeAndApproveMigration();
        VelaSettlement.MigrationAuthorization memory authorization = _migration(address(0), 1 ether);
        bytes memory authorizationSignature = _migrationSignature(authorization);
        vm.prank(guardian);
        settlement.cancelSuccessorVault();
        vm.warp(block.timestamp + settlement.MIGRATION_DELAY());
        vm.expectRevert(VelaSettlement.NoMigrationProposal.selector);
        settlement.executeMigration(authorization, authorizationSignature);
        assertEq(address(settlement).balance, 1 ether);
    }

    function test_roleSeparationAndOwnerRenunciationSafety() public {
        vm.expectRevert(VelaSettlement.RoleCollision.selector);
        settlement.setGuardian(operator);
        vm.expectRevert(VelaSettlement.RoleCollision.selector);
        settlement.proposeOperator(guardian);
        vm.expectRevert(VelaSettlement.OwnershipRenunciationDisabled.selector);
        settlement.renounceOwnership();
    }
}

contract SettlementHandler is Test {
    VelaSettlement public settlement;
    HardeningERC20 public token;
    uint256 public expectedEth;
    uint256 public expectedToken;

    constructor(VelaSettlement _settlement, HardeningERC20 _token) {
        settlement = _settlement;
        token = _token;
    }

    function depositETH(uint256 amount, bytes32 accountId) external {
        amount = bound(amount, 1, 10 ether);
        if (accountId == bytes32(0)) accountId = bytes32(uint256(1));
        vm.deal(address(this), amount);
        settlement.depositETH{ value: amount }(accountId);
        expectedEth += amount;
    }

    function depositToken(uint256 amount, bytes32 accountId) external {
        amount = bound(amount, 1, 100e18);
        if (accountId == bytes32(0)) accountId = bytes32(uint256(1));
        token.approve(address(settlement), amount);
        settlement.depositToken(accountId, address(token), amount);
        expectedToken += amount;
    }
}

contract VelaSettlementInvariantTest is Test {
    VelaSettlement settlement;
    HardeningERC20 token;
    SettlementHandler handler;

    function setUp() public {
        settlement = new VelaSettlement(address(0xA11CE), keccak256("invariant"));
        token = new HardeningERC20();
        settlement.setSupportedAsset(address(token), true);
        handler = new SettlementHandler(settlement, token);
        token.transfer(address(handler), 500_000e18);
        targetContract(address(handler));
    }

    function invariant_omnibusEthBalanceConserved() public view {
        assertEq(address(settlement).balance, handler.expectedEth());
        assertEq(settlement.totalDeposited(address(0)), handler.expectedEth());
    }

    function invariant_omnibusTokenBalanceConserved() public view {
        assertEq(token.balanceOf(address(settlement)), handler.expectedToken());
        assertEq(settlement.totalDeposited(address(token)), handler.expectedToken());
    }
}
