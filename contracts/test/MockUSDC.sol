// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import "@openzeppelin/contracts/token/ERC20/ERC20.sol";

/// @dev Local integration fixture only. Never register as a hosted asset.
contract MockUSDC is ERC20 {
    constructor() ERC20("Local Test USDC", "tUSDC") {
        _mint(msg.sender, 1_000_000_000);
    }

    function decimals() public pure override returns (uint8) {
        return 6;
    }
}
