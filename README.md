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
ghmd tx compute store contract.wasm.gz --from a --gas 1000000 --gas-prices 0.25ughm
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

## execute  token contract

#### deposit

```bash
ghmd tx compute execute <contract_address> '{"deposit":{}}' --amount 1000000ughm --from <account>
```

#### Redeem

```bash
ghmd tx compute execute <contract-address> '{"redeem": {"amount": "<amount_in_smallest_denom_of_token>"}}' --from <account>
```

#### Transfer

```bash
'{"transfer":{"amount":"<string>","recipient":"<address_string>"}}'
```

#### Send

```bash
'{"send":{"amount": <string>, "recipient": <string>}}'
```

#### BatchTransfer

```bash
'{"batch_transfer":{"actions":[{"amount": <string>, "recipient": <string>}]}}'
```

#### BatchSend

```bash
'{"batch_send":{"actions":[{"amount":<string>, "recipient":<string>}]}}'
```

#### Burn

```bash
'{"burn":{"amount": <string>}}'
```

#### RegisterReceive

```bash
'{"register_receive": {"code_hash": <string>}}'
```

#### CreateViewingKey

```bash
'{"create_viewing_key":{"entropy": <string>}'
```

#### SetViewingKey

```bash
ghmd tx compute execute <contract-address> '{"set_viewing_key": {"key": "<your_key>"}}' --from <account>
```

#### IncreaseAllowance

```bash
'{"increase_allowance":{"spender": <string>, "amount": <striong>}'
```

#### DecreaseAllowance

```bash
'{"decrease_allowance":{"spender": <string>,"amount":<string>}}'
```

#### TransferFrom

```bash
'{"transfer_from":{"amount":<string>, "owner":<string>, "recipient":<string>}}'
```

#### SendFrom

```bash
'{"send_from":{"amount":<string>, "owner":<string>, "recipient":<string>}'
```

#### BatchTransferFrom

```bash
'{"batch_transfer_from":{"actions":[{"amount":<string>, "owner":<string>, "recipient":<string>}]}'
```

#### BatchSendFrom

```bash
'{"batch_send_from":{"actions":[{"amount":<string>, "owner":<string>, "recipient":<string>}]}'
```

#### BurnFrom

```bash
'{"burn_from":{"amount":"<string>"", "owner":"<string>"}'
```

#### BatchBurnFrom


```bash
'{"batch_burn_from":{"actions":[{"amount":"<string>"", "owner":"<string>"}]}'
```


#### Mint


```bash
'{"mint":{"amount":"<string>","recipient":"<string>"}}'
```


#### BatchMint


```bash
'{"batch_mint":{"actions":[{"amount":"<string>","recipient":"<string>"}]}}'
```


#### ChangeAdmin

```bash
'{"change_admin":{"address":"<str>"}}'
```

#### SetContractStatus

```bash
'{"set_contract_status":{"level":"<string>"}}'
// normal_run
// stop_all_but_redeems
// stop_all
```

#### AddMinters

```bash
'{"add_minters":{"minters":["str1","str2"]}}'
```

#### RemoveMinters

```bash
'{"remove_minters":{"minters":["str1","str2"]}}'
```

#### SetMinters

```bash
'{"set_minters":{"minters":["str1","str2"]}}'
```

#### RevokePermit


```bash
'{"revoke_permit":{"permit_name":"<string>"}}'
```

## query  token contract info

#### TokenInfo


```bash
'{"token_info":{}}'
```


#### TokenConfig

```bash
ghmd q compute query <contract-address> '{"token_config": {}}'
```

#### ContractStatus


```bash
'{"contract_status":{}}'
```


#### ExchangeRate

```bash
ghmd q compute query <contract-address> '{"exchange_rate": {}}'
```

#### Minters


```bash
'{"minters":{}}'
```


#### WithPermit


```bash
// todo
'{"with_permit":{"permit":{},"query":""}}'
```


####  Balance

```bash
'{"balance":{"address":"<str>","key":"str"}}'
```

#### TransferHistory


```bash
'{"transfer_history":{"address":"<str>","key":"<str>","page_size":<int>}}'
```


#### TransactionHistory


```bash
'{"transaction_history":{"address":"<str>","key":"<str>","page_size":<int>}'
```


#### Allowance


```bash
'{"allowance":{"key":"<string>","owner":"<string>","spender":"<string>"}}'
```

