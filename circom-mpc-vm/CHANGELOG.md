# Changelog

## [0.1.0](https://github.com/TaceoLabs/collaborative-circom/compare/circom-mpc-vm-v0.0.1...circom-mpc-vm-v0.1.0) (2024-06-14)


### Features

* added better assert message when assert fails ([ae0d8be](https://github.com/TaceoLabs/collaborative-circom/commit/ae0d8be33307f3db6f5c179069ffeed38f61abbb))
* added pow/mod opcodes for plain VM ([#50](https://github.com/TaceoLabs/collaborative-circom/issues/50)) ([eb6e55c](https://github.com/TaceoLabs/collaborative-circom/commit/eb6e55c5fdf5e650ba7cdab52acab0b4af392615))
* fixed poseidonex_test ([f119394](https://github.com/TaceoLabs/collaborative-circom/commit/f1193948e1edbed19be7d9684b6f96a0e83d3045))
* implement eq/neq in mpc vm ([1e32551](https://github.com/TaceoLabs/collaborative-circom/commit/1e3255108578635ac869a564a6fcf5fab854fb03))
* implement shift right for public shift values ([7db3730](https://github.com/TaceoLabs/collaborative-circom/commit/7db3730d02624ec2f28dfd9d93f6bac174b88ff6))
* implemented plain/aby3 cmux and bool not for shared if handling ([e5701aa](https://github.com/TaceoLabs/collaborative-circom/commit/e5701aa8d967ab9d111556c8dfba3eeacfda4782))
* integrate witness extension via MPC VM into CLI binary ([f526081](https://github.com/TaceoLabs/collaborative-circom/commit/f526081a01e3faa6b48fb463f3690f968218a1a4))
* mpc accelerator first draft ([#79](https://github.com/TaceoLabs/collaborative-circom/issues/79)) ([5f2709b](https://github.com/TaceoLabs/collaborative-circom/commit/5f2709b2e56277328180f9990f1f21c77cdac06e))
* public inputs support ([#76](https://github.com/TaceoLabs/collaborative-circom/issues/76)) ([07cf260](https://github.com/TaceoLabs/collaborative-circom/commit/07cf26007285822ba42e8dce2439f676a2cf08ef))
* shared control flow test working for single return values ([6f4aabb](https://github.com/TaceoLabs/collaborative-circom/commit/6f4aabb3a842d47e148343a6b5e0c5b6d27b9b31))
* shared_control_flow arrays working except loops ([15cdecf](https://github.com/TaceoLabs/collaborative-circom/commit/15cdecf5d4dc6d0400367856a48f2571925745c3))
* VM if logic first draft ([cb9e525](https://github.com/TaceoLabs/collaborative-circom/commit/cb9e525e8ff4d96fb18a73a59589c09fcb756dff))


### Bug Fixes

* ab3 is_shared function + fixed a typo in cmux ([c6e4576](https://github.com/TaceoLabs/collaborative-circom/commit/c6e4576ac22de7569a6433e2dc862783c3bb02e2))
* correct handling of is_zero in binary MPC protocol ([432326e](https://github.com/TaceoLabs/collaborative-circom/commit/432326e9f2c24bca7a3a2f795711d677d1d37503))
* fixed a bug that sub components were not invoked when they did not have inputs (mux test cases) ([825b8e3](https://github.com/TaceoLabs/collaborative-circom/commit/825b8e3d78e4e9702c40b1e5db16faf41caa1f28))
* fixed array as paramters and return val for functions (escalarmulw4table_test) ([8f38648](https://github.com/TaceoLabs/collaborative-circom/commit/8f386487a40de20951d2124ed10d2ee76876e9bd))
* fixed iszero for aby3 ([244072a](https://github.com/TaceoLabs/collaborative-circom/commit/244072a1c5f98501dc8ba8003684db792fda92db))
* fixed smt and sha test cases (signal offset of components fixed) ([5442507](https://github.com/TaceoLabs/collaborative-circom/commit/54425070d5af1cdbca092fc365bdd2f66218b89b))
* missing call to bool_or ([d1a3bb1](https://github.com/TaceoLabs/collaborative-circom/commit/d1a3bb13bc08a711d248fa65b47d8c68b49878e6))
* slightly better error message for internal assertions ([75d51be](https://github.com/TaceoLabs/collaborative-circom/commit/75d51bee43c92f79916e3ecac047e198e63e9a96))