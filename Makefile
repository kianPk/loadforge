.PHONY: install test
install:
	forge install foundry-rs/forge-std OpenZeppelin/openzeppelin-contracts@v5.0.2 --no-commit
test:
	forge test -vv
