// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import "forge-std/Test.sol";
import "../src/VelaSettlement.sol";
import "@openzeppelin/contracts/token/ERC20/ERC20.sol";

contract MockERC20 is ERC20 {
    constructor() ERC20("Mock", "MCK") {
        _mint(msg.sender, 1_000_000e18);
    }
}

contract VelaSettlementTest is Test {
    VelaSettlement settlement;
    MockERC20 token;

    uint256 constant OPERATOR_KEY = 0xA11CE;
    address operator = vm.addr(OPERATOR_KEY);
    address user = address(0xBEEF);
    address destination = address(0xD357);
    address relayer = address(0xAE1A7);
    bytes32 accountId = keccak256("ledger-account-1");

    function setUp() public {
        settlement = new VelaSettlement(operator, keccak256("test-source-commit"));
        token = new MockERC20();
        settlement.setSupportedAsset(address(token), true);
        vm.deal(user, 10 ether);
        token.transfer(user, 1000e18);
    }

    function _withdrawal(address asset, uint256 amount, uint256 nonce)
        internal
        view
        returns (VelaSettlement.Withdrawal memory w)
    {
        w = VelaSettlement.Withdrawal({
            requestId: bytes16(keccak256(abi.encode("request", nonce))),
            destination: destination,
            asset: asset,
            amount: amount,
            nonce: nonce,
            deadline: block.timestamp + 1 hours,
            policyHash: keccak256("policy-v3"),
            approverHash: keccak256("approval-v3")
        });
    }

    function _signature(VelaSettlement.Withdrawal memory w) internal view returns (bytes memory) {
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(OPERATOR_KEY, settlement.withdrawalDigest(w));
        return abi.encodePacked(r, s, v);
    }

    function test_depositETHEmitsOpaqueAccountIdentityWithoutCreatingUserBalance() public {
        vm.recordLogs();
        vm.prank(user);
        settlement.depositETH{ value: 1 ether }(accountId);

        assertEq(address(settlement).balance, 1 ether);
        assertEq(settlement.totalDeposited(address(0)), 1 ether);
        assertEq(settlement.depositNonce(), 1);

        Vm.Log[] memory logs = vm.getRecordedLogs();
        assertEq(logs.length, 1);
        assertEq(logs[0].topics[0], keccak256("Deposited(bytes32,address,bytes32,address,uint256)"));
        assertEq(
            logs[0].topics[1], keccak256(abi.encode(block.chainid, address(settlement), user, accountId, address(0), 0))
        );
        assertEq(address(uint160(uint256(logs[0].topics[2]))), user);
        assertEq(logs[0].topics[3], accountId);
        (address asset, uint256 amount) = abi.decode(logs[0].data, (address, uint256));
        assertEq(asset, address(0));
        assertEq(amount, 1 ether);
    }

    function test_depositTokenRequiresExactReceipt() public {
        vm.startPrank(user);
        token.approve(address(settlement), 10e18);
        settlement.depositToken(accountId, address(token), 10e18);
        vm.stopPrank();
        assertEq(token.balanceOf(address(settlement)), 10e18);
        assertEq(settlement.totalDeposited(address(token)), 10e18);
    }

    function test_depositRejectsZeroOpaqueAccountId() public {
        vm.prank(user);
        vm.expectRevert(VelaSettlement.ZeroAccountId.selector);
        settlement.depositETH{ value: 1 ether }(bytes32(0));
    }

    function test_relayedWithdrawalPaysOnlySignedDestination() public {
        vm.prank(user);
        settlement.depositETH{ value: 1 ether }(accountId);
        VelaSettlement.Withdrawal memory w = _withdrawal(address(0), 0.4 ether, 1);

        vm.prank(relayer);
        settlement.executeWithdrawal(w, _signature(w));

        assertEq(destination.balance, 0.4 ether);
        assertEq(relayer.balance, 0);
        assertEq(address(settlement).balance, 0.6 ether);
        assertTrue(settlement.usedWithdrawalRequestIds(w.requestId));
        assertTrue(settlement.usedWithdrawNonces(w.nonce));
    }

    function test_authorizedBuyerCanWithdrawDepositedSellerAsset() public {
        vm.startPrank(user);
        token.approve(address(settlement), 10e18);
        settlement.depositToken(accountId, address(token), 10e18);
        vm.stopPrank();

        VelaSettlement.Withdrawal memory w = _withdrawal(address(token), 6e18, 2);
        settlement.executeWithdrawal(w, _signature(w));
        assertEq(token.balanceOf(destination), 6e18);
        assertEq(token.balanceOf(address(settlement)), 4e18);
    }

    function test_noLegacyPerUserBalanceOrEmergencyExitSelectors() public {
        (bool balanceOk,) =
            address(settlement).staticcall(abi.encodeWithSignature("getBalance(address,address)", user, address(0)));
        (bool exitOk,) = address(settlement).call(abi.encodeWithSignature("initiateEmergencyExit(address)", address(0)));
        assertFalse(balanceOk);
        assertFalse(exitOk);
    }

    function test_requestIdReplayRejectedWithDifferentNonce() public {
        vm.prank(user);
        settlement.depositETH{ value: 2 ether }(accountId);
        VelaSettlement.Withdrawal memory first = _withdrawal(address(0), 0.5 ether, 7);
        settlement.executeWithdrawal(first, _signature(first));

        VelaSettlement.Withdrawal memory replay = _withdrawal(address(0), 0.5 ether, 8);
        replay.requestId = first.requestId;
        bytes memory replaySignature = _signature(replay);
        vm.expectRevert(VelaSettlement.WithdrawalRequestAlreadyUsed.selector);
        settlement.executeWithdrawal(replay, replaySignature);
    }

    function test_globalNonceReplayRejectedWithDifferentRequestId() public {
        vm.prank(user);
        settlement.depositETH{ value: 2 ether }(accountId);
        VelaSettlement.Withdrawal memory first = _withdrawal(address(0), 0.5 ether, 9);
        settlement.executeWithdrawal(first, _signature(first));

        VelaSettlement.Withdrawal memory replay = _withdrawal(address(0), 0.5 ether, 9);
        replay.requestId = bytes16(keccak256("different"));
        bytes memory replaySignature = _signature(replay);
        vm.expectRevert(VelaSettlement.NonceAlreadyUsed.selector);
        settlement.executeWithdrawal(replay, replaySignature);
    }

    function test_signatureBindsDestinationAssetAmountDeadlineAndEvidence() public {
        vm.prank(user);
        settlement.depositETH{ value: 2 ether }(accountId);
        VelaSettlement.Withdrawal memory original = _withdrawal(address(0), 0.5 ether, 10);
        bytes memory signature = _signature(original);

        VelaSettlement.Withdrawal memory changed = original;
        changed.destination = address(0xBAD);
        vm.expectRevert(VelaSettlement.InvalidSignature.selector);
        settlement.executeWithdrawal(changed, signature);
        changed = original;
        changed.amount += 1;
        vm.expectRevert(VelaSettlement.InvalidSignature.selector);
        settlement.executeWithdrawal(changed, signature);
        changed = original;
        changed.policyHash = keccak256("substituted");
        vm.expectRevert(VelaSettlement.InvalidSignature.selector);
        settlement.executeWithdrawal(changed, signature);
    }

    function test_expiredAuthorizationDoesNotConsumeReplayGuards() public {
        vm.prank(user);
        settlement.depositETH{ value: 1 ether }(accountId);
        VelaSettlement.Withdrawal memory w = _withdrawal(address(0), 0.5 ether, 11);
        bytes memory signature = _signature(w);
        vm.warp(w.deadline + 1);
        vm.expectRevert(VelaSettlement.WithdrawalExpired.selector);
        settlement.executeWithdrawal(w, signature);
        assertFalse(settlement.usedWithdrawalRequestIds(w.requestId));
        assertFalse(settlement.usedWithdrawNonces(w.nonce));
    }

    function test_crossContractAndCrossChainReplayRejected() public {
        vm.prank(user);
        settlement.depositETH{ value: 2 ether }(accountId);
        VelaSettlement.Withdrawal memory w = _withdrawal(address(0), 0.5 ether, 12);
        bytes memory signature = _signature(w);

        VelaSettlement other = new VelaSettlement(operator, keccak256("other"));
        vm.deal(address(other), 1 ether);
        vm.expectRevert(VelaSettlement.InvalidSignature.selector);
        other.executeWithdrawal(w, signature);

        vm.chainId(block.chainid + 1);
        vm.expectRevert(VelaSettlement.InvalidSignature.selector);
        settlement.executeWithdrawal(w, signature);
    }

    function test_eip712V3ShapeIsStable() public view {
        (bytes1 fields, string memory name, string memory version, uint256 chainId, address verifyingContract,,) =
            settlement.eip712Domain();
        assertEq(fields, hex"0f");
        assertEq(name, "VelaSettlement");
        assertEq(version, "3");
        assertEq(chainId, block.chainid);
        assertEq(verifyingContract, address(settlement));
        assertEq(
            settlement.WITHDRAWAL_TYPEHASH(),
            keccak256(
                "Withdrawal(bytes16 requestId,address destination,address asset,uint256 amount,uint256 nonce,uint256 deadline,bytes32 policyHash,bytes32 approverHash)"
            )
        );
    }

    /// @dev Cross-language vector for the ledger's V3 authorization digest.
    function test_withdrawalDigestV3FixedVector() public {
        vm.chainId(84_532);
        address fixedDeployment = address(bytes20(hex"111122223333444455556666777788889999aaaa"));
        deployCodeTo(
            "VelaSettlement.sol:VelaSettlement",
            abi.encode(operator, keccak256("fixed-vector-source-commit")),
            fixedDeployment
        );
        VelaSettlement fixedSettlement = VelaSettlement(payable(fixedDeployment));
        VelaSettlement.Withdrawal memory w = VelaSettlement.Withdrawal({
            requestId: hex"00112233445566778899aabbccddeeff",
            destination: address(0xD357),
            asset: address(0),
            amount: 0.5 ether,
            nonce: 42,
            deadline: 2_000_000_000,
            policyHash: keccak256("policy-vector"),
            approverHash: keccak256("approver-vector")
        });

        assertEq(
            fixedSettlement.withdrawalDigest(w), 0x21207c5c9bc8aa5999379b92ff1ac80a4a48daff0a0f10763771677aa4b2b68b
        );
    }
}
