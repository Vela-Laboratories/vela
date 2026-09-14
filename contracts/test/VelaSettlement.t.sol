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
    event WithdrawalExecuted(
        bytes16 indexed requestId,
        address indexed user,
        address indexed destination,
        address asset,
        uint256 amount,
        uint256 nonce,
        uint256 deadline,
        bytes32 policyHash,
        bytes32 approverHash
    );

    VelaSettlement settlement;
    MockERC20 token;

    uint256 operatorKey = 0xA11CE;
    address operator;
    address user = address(0xBEEF);
    address destination = address(0xD357);
    address relayer = address(0xAE1A7);

    function setUp() public {
        operator = vm.addr(operatorKey);
        settlement = new VelaSettlement(operator);
        token = new MockERC20();

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
            user: user,
            destination: destination,
            asset: asset,
            amount: amount,
            nonce: nonce,
            deadline: block.timestamp + 1 hours,
            policyHash: keccak256("withdrawal-policy-v1"),
            approverHash: keccak256("approver-evidence-v1")
        });
    }

    function _digest(VelaSettlement target, VelaSettlement.Withdrawal memory w) internal view returns (bytes32) {
        return target.withdrawalDigest(w);
    }

    function _signature(VelaSettlement target, VelaSettlement.Withdrawal memory w)
        internal
        view
        returns (bytes memory)
    {
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(operatorKey, _digest(target, w));
        return abi.encodePacked(r, s, v);
    }

    function _execute(VelaSettlement target, VelaSettlement.Withdrawal memory w, bytes memory signature) internal {
        target.executeWithdrawal(w, signature);
    }

    function test_depositETH() public {
        vm.recordLogs();
        vm.prank(user);
        settlement.depositETH{ value: 1 ether }();
        assertEq(settlement.getBalance(user, address(0)), 1 ether);
        assertEq(settlement.depositNonce(), 1);

        Vm.Log[] memory logs = vm.getRecordedLogs();
        assertEq(logs.length, 1);
        assertEq(logs[0].topics.length, 4);
        assertEq(logs[0].topics[0], keccak256("Deposited(bytes32,address,address,uint256)"));
        assertEq(logs[0].topics[1], keccak256(abi.encode(block.chainid, address(settlement), user, address(0), 0)));
        assertEq(address(uint160(uint256(logs[0].topics[2]))), user);
        assertEq(address(uint160(uint256(logs[0].topics[3]))), address(0));
        assertEq(abi.decode(logs[0].data, (uint256)), 1 ether);
    }

    function test_depositIdsAreUniqueAcrossDepositsAndDeployments() public {
        vm.recordLogs();
        vm.startPrank(user);
        settlement.depositETH{ value: 1 ether }();
        settlement.depositETH{ value: 1 ether }();
        vm.stopPrank();
        Vm.Log[] memory firstLogs = vm.getRecordedLogs();
        assertNotEq(firstLogs[0].topics[1], firstLogs[1].topics[1]);

        VelaSettlement other = new VelaSettlement(operator);
        vm.recordLogs();
        vm.prank(user);
        other.depositETH{ value: 1 ether }();
        Vm.Log[] memory otherLogs = vm.getRecordedLogs();
        assertNotEq(firstLogs[0].topics[1], otherLogs[0].topics[1]);
    }

    function test_initiateAndExecuteEmergencyExit() public {
        vm.prank(user);
        settlement.depositETH{ value: 1 ether }();

        vm.prank(user);
        settlement.initiateEmergencyExit(address(0));

        vm.warp(block.timestamp + 7 days);

        uint256 balBefore = user.balance;
        vm.prank(user);
        settlement.executeEmergencyExit(address(0));

        assertEq(settlement.getBalance(user, address(0)), 0);
        assertEq(user.balance, balBefore + 1 ether);
    }

    function test_emergencyExitRevertsBeforeTimelock() public {
        vm.prank(user);
        settlement.depositETH{ value: 1 ether }();

        vm.prank(user);
        settlement.initiateEmergencyExit(address(0));

        vm.warp(block.timestamp + 6 days);

        vm.prank(user);
        vm.expectRevert(VelaSettlement.TimelockActive.selector);
        settlement.executeEmergencyExit(address(0));
    }

    function test_executeWithdrawalCanBeRelayedAndPaysSignedDestination() public {
        vm.prank(user);
        settlement.depositETH{ value: 1 ether }();
        VelaSettlement.Withdrawal memory w = _withdrawal(address(0), 0.5 ether, 1);
        bytes memory signature = _signature(settlement, w);

        vm.expectEmit(true, true, true, true, address(settlement));
        emit WithdrawalExecuted(
            w.requestId, w.user, w.destination, w.asset, w.amount, w.nonce, w.deadline, w.policyHash, w.approverHash
        );
        vm.prank(relayer);
        _execute(settlement, w, signature);

        assertEq(settlement.getBalance(user, address(0)), 0.5 ether);
        assertEq(destination.balance, 0.5 ether);
        assertEq(relayer.balance, 0);
        assertTrue(settlement.usedWithdrawalRequestIds(w.requestId));
        assertTrue(settlement.usedWithdrawNonces(user, 1));
    }

    function test_executeWithdrawalAcceptsCustodyRecoveryIdZeroOrOne() public {
        vm.prank(user);
        settlement.depositETH{ value: 1 ether }();
        VelaSettlement.Withdrawal memory w = _withdrawal(address(0), 0.5 ether, 101);
        bytes memory signature = _signature(settlement, w);
        signature[64] = bytes1(uint8(signature[64]) - 27);

        vm.prank(relayer);
        _execute(settlement, w, signature);

        assertEq(destination.balance, 0.5 ether);
        assertTrue(settlement.usedWithdrawalRequestIds(w.requestId));
    }

    function test_executeWithdrawalTransfersErc20ToSignedDestination() public {
        vm.startPrank(user);
        token.approve(address(settlement), 10e18);
        settlement.depositToken(address(token), 10e18);
        vm.stopPrank();
        VelaSettlement.Withdrawal memory w = _withdrawal(address(token), 4e18, 2);

        vm.prank(relayer);
        _execute(settlement, w, _signature(settlement, w));

        assertEq(settlement.getBalance(user, address(token)), 6e18);
        assertEq(token.balanceOf(destination), 4e18);
        assertEq(token.balanceOf(relayer), 0);
    }

    function test_executeWithdrawalRevertsInvalidSignature() public {
        vm.prank(user);
        settlement.depositETH{ value: 1 ether }();
        VelaSettlement.Withdrawal memory w = _withdrawal(address(0), 0.5 ether, 1);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(0xBAD, _digest(settlement, w));

        vm.expectRevert(VelaSettlement.InvalidSignature.selector);
        _execute(settlement, w, abi.encodePacked(r, s, v));
    }

    function test_legacyWithdrawSelectorIsDisabled() public {
        vm.prank(user);
        settlement.depositETH{ value: 1 ether }();

        vm.prank(user);
        (bool ok,) = address(settlement)
            .call(
                abi.encodeWithSignature("withdraw(address,uint256,uint256,bytes)", address(0), 0.5 ether, 1, bytes(""))
            );

        assertFalse(ok);
        assertEq(settlement.getBalance(user, address(0)), 1 ether);
    }

    function test_executeWithdrawalRevertsInsufficientBalance() public {
        vm.prank(user);
        settlement.depositETH{ value: 0.1 ether }();
        VelaSettlement.Withdrawal memory w = _withdrawal(address(0), 1 ether, 1);
        bytes memory signature = _signature(settlement, w);

        vm.expectRevert(VelaSettlement.InsufficientBalance.selector);
        _execute(settlement, w, signature);
    }

    function test_requestIdReplayIsRejectedEvenWithDifferentNonce() public {
        vm.prank(user);
        settlement.depositETH{ value: 2 ether }();
        VelaSettlement.Withdrawal memory first = _withdrawal(address(0), 0.5 ether, 1);
        _execute(settlement, first, _signature(settlement, first));

        VelaSettlement.Withdrawal memory replay = _withdrawal(address(0), 0.5 ether, 2);
        replay.requestId = first.requestId;
        bytes memory replaySignature = _signature(settlement, replay);
        vm.expectRevert(VelaSettlement.WithdrawalRequestAlreadyUsed.selector);
        _execute(settlement, replay, replaySignature);
    }

    function test_userNonceReplayIsRejectedEvenWithDifferentRequestId() public {
        vm.prank(user);
        settlement.depositETH{ value: 2 ether }();
        VelaSettlement.Withdrawal memory first = _withdrawal(address(0), 0.5 ether, 7);
        _execute(settlement, first, _signature(settlement, first));

        VelaSettlement.Withdrawal memory replay = _withdrawal(address(0), 0.5 ether, 7);
        replay.requestId = bytes16(keccak256("different request"));
        bytes memory replaySignature = _signature(settlement, replay);
        vm.expectRevert(VelaSettlement.NonceAlreadyUsed.selector);
        _execute(settlement, replay, replaySignature);
    }

    function test_sameNonceIsIndependentAcrossUsers() public {
        address otherUser = address(0xCAFE);
        vm.deal(otherUser, 1 ether);
        vm.prank(user);
        settlement.depositETH{ value: 1 ether }();
        vm.prank(otherUser);
        settlement.depositETH{ value: 1 ether }();

        VelaSettlement.Withdrawal memory first = _withdrawal(address(0), 0.25 ether, 9);
        _execute(settlement, first, _signature(settlement, first));
        VelaSettlement.Withdrawal memory second = _withdrawal(address(0), 0.25 ether, 9);
        second.requestId = bytes16(keccak256("other-user-request"));
        second.user = otherUser;
        _execute(settlement, second, _signature(settlement, second));

        assertTrue(settlement.usedWithdrawNonces(user, 9));
        assertTrue(settlement.usedWithdrawNonces(otherUser, 9));
    }

    function test_expiredWithdrawalIsRejectedWithoutConsumingReplayGuards() public {
        vm.prank(user);
        settlement.depositETH{ value: 1 ether }();
        VelaSettlement.Withdrawal memory w = _withdrawal(address(0), 0.5 ether, 1);
        bytes memory signature = _signature(settlement, w);
        vm.warp(w.deadline + 1);

        vm.expectRevert(VelaSettlement.WithdrawalExpired.selector);
        _execute(settlement, w, signature);
        assertFalse(settlement.usedWithdrawalRequestIds(w.requestId));
        assertFalse(settlement.usedWithdrawNonces(user, w.nonce));
    }

    function test_withdrawalIsValidAtExactDeadline() public {
        vm.prank(user);
        settlement.depositETH{ value: 1 ether }();
        VelaSettlement.Withdrawal memory w = _withdrawal(address(0), 0.5 ether, 1);
        bytes memory signature = _signature(settlement, w);
        vm.warp(w.deadline);

        _execute(settlement, w, signature);
        assertEq(destination.balance, 0.5 ether);
    }

    function test_zeroRequestIdAndDestinationAreRejected() public {
        vm.prank(user);
        settlement.depositETH{ value: 1 ether }();
        VelaSettlement.Withdrawal memory w = _withdrawal(address(0), 0.5 ether, 1);
        w.requestId = bytes16(0);
        bytes memory zeroRequestSignature = _signature(settlement, w);
        vm.expectRevert(VelaSettlement.InvalidWithdrawalRequestId.selector);
        _execute(settlement, w, zeroRequestSignature);

        w.requestId = bytes16(keccak256("valid"));
        w.destination = address(0);
        bytes memory zeroDestinationSignature = _signature(settlement, w);
        vm.expectRevert(VelaSettlement.ZeroAddress.selector);
        _execute(settlement, w, zeroDestinationSignature);
    }

    function test_signedDestinationPolicyAndApproverCannotBeSubstituted() public {
        vm.prank(user);
        settlement.depositETH{ value: 2 ether }();
        VelaSettlement.Withdrawal memory signed = _withdrawal(address(0), 0.5 ether, 1);
        bytes memory signature = _signature(settlement, signed);

        VelaSettlement.Withdrawal memory changed = signed;
        changed.destination = address(0xBAD1);
        vm.expectRevert(VelaSettlement.InvalidSignature.selector);
        _execute(settlement, changed, signature);

        changed = signed;
        changed.policyHash = keccak256("different-policy");
        vm.expectRevert(VelaSettlement.InvalidSignature.selector);
        _execute(settlement, changed, signature);

        changed = signed;
        changed.approverHash = keccak256("different-approver");
        vm.expectRevert(VelaSettlement.InvalidSignature.selector);
        _execute(settlement, changed, signature);
    }

    function test_crossContractReplayIsRejectedByDomain() public {
        vm.prank(user);
        settlement.depositETH{ value: 1 ether }();
        VelaSettlement other = new VelaSettlement(operator);
        vm.prank(user);
        other.depositETH{ value: 1 ether }();
        VelaSettlement.Withdrawal memory w = _withdrawal(address(0), 0.5 ether, 42);
        bytes memory signature = _signature(settlement, w);

        vm.expectRevert(VelaSettlement.InvalidSignature.selector);
        _execute(other, w, signature);
    }

    function test_crossChainReplayIsRejectedByDomain() public {
        vm.prank(user);
        settlement.depositETH{ value: 1 ether }();
        VelaSettlement.Withdrawal memory w = _withdrawal(address(0), 0.5 ether, 42);
        bytes memory signature = _signature(settlement, w);
        vm.chainId(block.chainid + 1);

        vm.expectRevert(VelaSettlement.InvalidSignature.selector);
        _execute(settlement, w, signature);
    }

    function test_eip712DomainAndTypeHashAreStable() public view {
        (bytes1 fields, string memory name, string memory version, uint256 chainId, address verifyingContract,,) =
            settlement.eip712Domain();
        assertEq(fields, hex"0f");
        assertEq(name, "VelaSettlement");
        assertEq(version, "2");
        assertEq(chainId, block.chainid);
        assertEq(verifyingContract, address(settlement));
        assertEq(
            settlement.WITHDRAWAL_TYPEHASH(),
            keccak256(
                "Withdrawal(bytes16 requestId,address user,address destination,address asset,uint256 amount,uint256 nonce,uint256 deadline,bytes32 policyHash,bytes32 approverHash)"
            )
        );
    }

    /// @dev Cross-language test vector for broadcaster/signing implementations.
    function test_withdrawalDigestFixedVector() public {
        vm.chainId(84_532);
        address fixedDeployment = address(bytes20(hex"111122223333444455556666777788889999aaaa"));
        deployCodeTo("VelaSettlement.sol:VelaSettlement", abi.encode(operator), fixedDeployment);
        VelaSettlement fixedSettlement = VelaSettlement(payable(fixedDeployment));
        VelaSettlement.Withdrawal memory w = VelaSettlement.Withdrawal({
            requestId: hex"00112233445566778899aabbccddeeff",
            user: address(0xBEEF),
            destination: address(0xD357),
            asset: address(0),
            amount: 0.5 ether,
            nonce: 42,
            deadline: 2_000_000_000,
            policyHash: keccak256("policy-vector"),
            approverHash: keccak256("approver-vector")
        });

        assertEq(
            fixedSettlement.withdrawalDigest(w), 0xe4e370d5cee128859be93c6c83d94a246f2fb7996d6fc9499ea81e0277fc89a0
        );
    }

    function test_signatureMalleabilityRejected() public {
        vm.prank(user);
        settlement.depositETH{ value: 1 ether }();
        VelaSettlement.Withdrawal memory w = _withdrawal(address(0), 0.5 ether, 1);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(operatorKey, _digest(settlement, w));
        uint256 n = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141;
        bytes32 highS = bytes32(n - uint256(s));
        uint8 flippedV = v == 27 ? 28 : 27;

        vm.expectRevert(VelaSettlement.InvalidSignature.selector);
        _execute(settlement, w, abi.encodePacked(r, highS, flippedV));
    }
}
