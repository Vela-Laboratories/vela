// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import "forge-std/Test.sol";
import "../src/VelaSettlement.sol";

contract VelaSettlementMainnetForkTest is Test {
    address constant USDC = 0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48;
    uint256 constant OPERATOR_KEY = 0xA11CE;

    bool forkEnabled;
    VelaSettlement settlement;
    address operator;
    address user = address(0xBEEF);
    address destination = address(0xD357);

    function setUp() public {
        string memory rpc = vm.envOr("MAINNET_RPC_URL", string(""));
        if (bytes(rpc).length == 0) return;
        vm.createSelectFork(rpc);
        forkEnabled = true;
        operator = vm.addr(OPERATOR_KEY);
        settlement = new VelaSettlement(operator, keccak256("mainnet-fork-harness"));
        settlement.setSupportedAsset(USDC, true);
    }

    function test_mainnetUsdcDepositAndExactAuthorizedWithdrawal() public {
        if (!forkEnabled) return;
        uint256 amount = 100e6;
        deal(USDC, user, amount);
        vm.startPrank(user);
        IERC20(USDC).approve(address(settlement), amount);
        settlement.depositToken(keccak256("fork-account"), USDC, amount);
        vm.stopPrank();

        VelaSettlement.Withdrawal memory withdrawal = VelaSettlement.Withdrawal({
            requestId: bytes16(keccak256("fork-request")),
            destination: destination,
            asset: USDC,
            amount: amount,
            nonce: 1,
            deadline: block.timestamp + 1 hours,
            policyHash: keccak256("fork-policy"),
            approverHash: keccak256("fork-approval")
        });
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(OPERATOR_KEY, settlement.withdrawalDigest(withdrawal));
        settlement.executeWithdrawal(withdrawal, abi.encodePacked(r, s, v));

        assertEq(IERC20(USDC).balanceOf(destination), amount);
        assertEq(IERC20(USDC).balanceOf(address(settlement)), 0);
        assertTrue(settlement.usedWithdrawalRequestIds(withdrawal.requestId));
        assertTrue(settlement.usedWithdrawNonces(withdrawal.nonce));
    }
}
