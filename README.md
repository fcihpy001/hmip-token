# HMIP20

hmip20 smart contract for hermit network.

### download 

```bash
git clone https://github.com/HermitMatrixNetwork/hmip20.git
```

## before compile
```bash
apt update && apt install -y binaryen clang
```
## compile
```bash
cd himp20
cargo test

make compile-optimized
```
this commone will output  a file `contract.wasm.gz`.

## Usage of Hmip20 Contract

### storage contract code
```bash
ghmd tx compute store contract.wasm.gz --from a --gas 1000000 --gas-prices 0.25uGHM
```

use `ghmd q compute list-code`, find the number of upload contract

### instantiate a token contract
```bash
ghmd tx compute instantiate <code-id> \
   '{"name":"<your_token_name>","symbol":"<your_token_symbol>","admin":"<optional_admin_address_defaults_to_the_from_address>","decimals":<number_of_decimals>,"initial_balances":[{"address":"<address1>","amount":"<amount_for_address1>"}],"prng_seed":"<base64_encoded_string>","config":{"public_total_supply":<true_or_false>,"enable_deposit":<true_or_false>,"enable_redeem":<true_or_false>,"enable_mint":<true_or_false>,"enable_burn":<true_or_false>}}' \
    --label <token_label>  \
    --from <account>
```

#### config your contract 
```json
"config":{
    "public_total_supply":<true_or_false>,
    "enable_deposit":<true_or_false>,
    "enable_redeem":<true_or_false>,
    "enable_mint":<true_or_false>,
    "enable_burn":<true_or_false>
}
```

### execute  token contract

#### deposit

```bash
ghmd tx compute execute <contract_address> '{"deposit":{}}' --amount 1000000uGHM --from <account>
```

#### Redeem

```bash
ghmd tx compute 
```



#### Transfer



#### Send



#### BatchTransfer



#### BatchSend



#### Burn



#### RegisterReceive



#### CreateViewingKey



#### SetViewingKey

```bash
ghmd tx compute execute <contract-address> '{"create_viewing_key": {"entropy": "<random_phrase>"}}' --from <account>
```

#### IncreaseAllowance



#### DecreaseAllowance



#### TransferFrom



#### SendFrom



#### BatchTransferFrom



#### BatchSendFrom



#### BurnFrom



#### BatchBurnFrom



#### Mint



#### BatchMint



#### ChangeAdmin



#### SetContractStatus



#### AddMinters



#### RemoveMinters



#### SetMinters



#### RevokePermit



### query  token contract info

#### TokenInfo



#### TokenConfig



#### ContractStatus



#### ExchangeRate



#### Minters



#### WithPermit



####  Balance



#### TransferHistory



#### TransactionHistory



#### Allowance



