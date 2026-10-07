// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import "forge-std/Script.sol";
import "../src/VelaSettlement.sol";

contract Deploy is Script {
    function run() external {
        uint256 expectedChainId = vm.envUint("VELA_EXPECTED_CHAIN_ID");
        require(block.chainid == expectedChainId, "unexpected deployment chain");
        address operator = vm.envAddress("VELA_OPERATOR_ADDRESS");
        address owner = vm.envAddress("VELA_OWNER_ADDRESS");
        address guardian = vm.envAddress("VELA_GUARDIAN_ADDRESS");
        address usdc = vm.envAddress("VELA_USDC_ADDRESS");
        bytes32 sourceCommit = vm.envBytes32("VELA_SOURCE_COMMIT");
        require(operator != owner && operator != guardian && owner != guardian, "roles must be distinct");
        require(usdc.code.length > 0, "USDC must be a deployed contract");

        // Environment input keeps the signer key out of process arguments.
        if (vm.envExists("VELA_DEPLOYER_PRIVATE_KEY")) {
            vm.startBroadcast(vm.envUint("VELA_DEPLOYER_PRIVATE_KEY"));
        } else {
            vm.startBroadcast();
        }
        VelaSettlement settlement = new VelaSettlement(operator, sourceCommit);
        settlement.setSupportedAsset(usdc, true);
        settlement.setGuardian(guardian);
        if (owner != settlement.owner()) settlement.transferOwnership(owner);
        vm.stopBroadcast();

        console.log("VelaSettlement deployed to:", address(settlement));
        console.log("Chain ID:", block.chainid);
        console.log("Operator:", operator);
        console.log("Configured owner:", owner);
        console.log("Guardian:", guardian);
        console.log("USDC:", usdc);
        console.logBytes32(sourceCommit);
        console.logBytes32(address(settlement).codehash);
    }
}
