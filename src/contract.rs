use cosmwasm_std::{Api, Binary, Env, Extern, HandleResponse, HumanAddr, InitResponse, Querier, QueryResult, StdError, StdResult, Storage};
use hermit_toolkit::permit::{Permit, TokenPermissions, validate};
use crate::handle::{add_minters, change_admin, remove_minters, revoke_permit, set_contract_status, set_minters, try_batch_burn_from, try_batch_mint, try_batch_send, try_batch_send_from, try_batch_transfer, try_batch_transfer_from, try_burn, try_burn_from, try_create_key, try_decrease_allowance, try_deposit, try_increase_allowance, try_mint, try_redeem, try_register_receive, try_send, try_send_from, try_set_key, try_transfer, try_transfer_from};
use crate::msg::{ContractStatusLevel, HandleMsg, InitMsg, QueryMsg, QueryWithPermit, space_pad};
use crate::query::{query_allowance, query_balance, query_contract_status, query_exchange_rate, query_minters, query_token_config, query_token_info, query_transactions, query_transfers, viewing_keys_queries};
use crate::state::{Balances, Config, Constants, ReadonlyConfig};
use crate::tools::rand::sha_256;
use crate::transaction_history::store_mint;

/// We make sure that responses from `handle` are padded to a multiple of this size.
pub const RESPONSE_BLOCK_SIZE: usize = 256;
pub const PREFIX_REVOKED_PERMITS: &str = "revoked_permits";

pub fn init<S: Storage, A: Api, Q: Querier> (
    deps: &mut Extern<S,A,Q>,
    env: Env,
    msg: InitMsg,
) -> StdResult<InitResponse>{
    // Check name, symbol, decimals
    if !is_valid_name(&msg.name) {
        return Err(StdError::generic_err(
            "Name is not in the expected format (3-30 UTF-8 bytes)",
        ));
    }
    if !is_valid_symbol(&msg.symbol) {
        return Err(StdError::generic_err(
            "Ticker symbol is not in expected format [A-Z]{3,6}",
        ));
    }
    if msg.decimals > 18 {
        return Err(StdError::generic_err("Decimals must not exceed 18"));
    }

    let init_config = msg.config();
    let admin = msg.admin.unwrap_or(env.message.sender);
    let canon_admin = deps.api.canonical_address(&admin)?;

    let mut total_supply: u128 = 0;
    {
        let initial_balances = msg.initial_balances.unwrap_or_default();
        for balance in initial_balances {
            let balance_address = deps.api.canonical_address(&balance.address)?;
            let amount = balance.amount.u128();
            let mut balances = Balances::from_storage(&mut deps.storage);
            balances.set_account_balance(&balance_address, amount);
            if let Some(new_total_supply) = total_supply.checked_add(amount) {
                total_supply = new_total_supply;
            } else {
                return Err(StdError::generic_err(
                    "The sum of all initial balances exceeds the maximum possible total supply",
                ));
            }
            store_mint(
                &mut deps.storage,
                &canon_admin,
                &balance_address,
                balance.amount,
                msg.symbol.clone(),
                Some("Initial Balance".to_string()),
                &env.block,
            )?;
        }
    }

    let prng_seed_hashed = sha_256(&msg.prng_seed.0);

    let mut config = Config::from_storage(&mut deps.storage);
    config.set_constants(&Constants {
        name: msg.name,
        symbol: msg.symbol,
        decimals: msg.decimals,
        admin: admin.clone(),
        prng_seed: prng_seed_hashed.to_vec(),
        total_supply_is_public: init_config.public_total_supply(),
        deposit_is_enabled: init_config.deposit_enabled(),
        redeem_is_enabled: init_config.redeem_enabled(),
        mint_is_enabled: init_config.mint_enabled(),
        burn_is_enabled: init_config.burn_enabled(),
        contract_address: env.contract.address,
    })?;
    config.set_total_supply(total_supply);
    config.set_contract_status(ContractStatusLevel::NormalRun);
    let minters = if init_config.mint_enabled() {
        Vec::from([admin])
    } else {
        Vec::new()
    };
    config.set_minters(minters)?;

    Ok(InitResponse::default())
}

pub fn handle<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    msg: HandleMsg,
) -> StdResult<HandleResponse> {
    let contract_status = ReadonlyConfig::from_storage(&deps.storage).contract_status();

    match contract_status {
        ContractStatusLevel::StopAll | ContractStatusLevel::StopAllButRedeems => {
            let response = match msg {
                HandleMsg::SetContractStatus { level, .. } => set_contract_status(deps, env, level),
                HandleMsg::Redeem { amount, .. }
                if contract_status == ContractStatusLevel::StopAllButRedeems =>
                    {
                        try_redeem(deps, env, amount)
                    }
                _ => Err(StdError::generic_err(
                    "This contract is stopped and this action is not allowed",
                )),
            };
            return pad_response(response);
        }
        ContractStatusLevel::NormalRun => {} // If it's a normal run just continue
    }

    let response = match msg {
        // Native
        HandleMsg::Deposit { .. } => try_deposit(deps, env),
        HandleMsg::Redeem { amount, .. } => try_redeem(deps, env, amount),

        // Base
        HandleMsg::Transfer {
            recipient,
            amount,
            memo,
            ..
        } => try_transfer(deps, env, recipient, amount, memo),
        HandleMsg::Send {
            recipient,
            recipient_code_hash,
            amount,
            msg,
            memo,
            ..
        } => try_send(deps, env, recipient, recipient_code_hash, amount, memo, msg),
        HandleMsg::BatchTransfer { actions, .. } => try_batch_transfer(deps, env, actions),
        HandleMsg::BatchSend { actions, .. } => try_batch_send(deps, env, actions),
        HandleMsg::Burn { amount, memo, .. } => try_burn(deps, env, amount, memo),
        HandleMsg::RegisterReceive { code_hash, .. } => try_register_receive(deps, env, code_hash),
        HandleMsg::CreateViewingKey { entropy, .. } => try_create_key(deps, env, entropy),
        HandleMsg::SetViewingKey { key, .. } => try_set_key(deps, env, key),

        // Allowance
        HandleMsg::IncreaseAllowance {
            spender,
            amount,
            expiration,
            ..
        } => try_increase_allowance(deps, env, spender, amount, expiration),
        HandleMsg::DecreaseAllowance {
            spender,
            amount,
            expiration,
            ..
        } => try_decrease_allowance(deps, env, spender, amount, expiration),
        HandleMsg::TransferFrom {
            owner,
            recipient,
            amount,
            memo,
            ..
        } => try_transfer_from(deps, &env, &owner, &recipient, amount, memo),
        HandleMsg::SendFrom {
            owner,
            recipient,
            recipient_code_hash,
            amount,
            msg,
            memo,
            ..
        } => try_send_from(
            deps,
            env,
            owner,
            recipient,
            recipient_code_hash,
            amount,
            memo,
            msg,
        ),
        HandleMsg::BatchTransferFrom { actions, .. } => {
            try_batch_transfer_from(deps, &env, actions)
        }
        HandleMsg::BatchSendFrom { actions, .. } => try_batch_send_from(deps, env, actions),
        HandleMsg::BurnFrom {
            owner,
            amount,
            memo,
            ..
        } => try_burn_from(deps, &env, &owner, amount, memo),
        HandleMsg::BatchBurnFrom { actions, .. } => try_batch_burn_from(deps, &env, actions),

        // Mint
        HandleMsg::Mint {
            recipient,
            amount,
            memo,
            ..
        } => try_mint(deps, env, recipient, amount, memo),
        HandleMsg::BatchMint { actions, .. } => try_batch_mint(deps, env, actions),

        // Other
        HandleMsg::ChangeAdmin { address, .. } => change_admin(deps, env, address),
        HandleMsg::SetContractStatus { level, .. } => set_contract_status(deps, env, level),
        HandleMsg::AddMinters { minters, .. } => add_minters(deps, env, minters),
        HandleMsg::RemoveMinters { minters, .. } => remove_minters(deps, env, minters),
        HandleMsg::SetMinters { minters, .. } => set_minters(deps, env, minters),
        HandleMsg::RevokePermit { permit_name, .. } => revoke_permit(deps, env, permit_name),
    };

    pad_response(response)
}

pub fn query<S: Storage, A: Api, Q: Querier>(deps: &Extern<S, A, Q>, msg: QueryMsg) -> QueryResult {
    match msg {
        QueryMsg::TokenInfo {} => query_token_info(&deps.storage),
        QueryMsg::TokenConfig {} => query_token_config(&deps.storage),
        QueryMsg::ContractStatus {} => query_contract_status(&deps.storage),
        QueryMsg::ExchangeRate {} => query_exchange_rate(&deps.storage),
        QueryMsg::Minters { .. } => query_minters(deps),
        QueryMsg::WithPermit { permit, query } => permit_queries(deps, permit, query),
        _ => viewing_keys_queries(deps, msg),
    }
}
fn permit_queries<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    permit: Permit,
    query: QueryWithPermit,
) -> Result<Binary, StdError> {
    // Validate permit content
    let token_address = ReadonlyConfig::from_storage(&deps.storage)
        .constants()?
        .contract_address;

    let account = HumanAddr(validate(
        deps,
        PREFIX_REVOKED_PERMITS,
        &permit,
        token_address,
        None,
    )?);

    // Permit validated! We can now execute the query.
    match query {
        QueryWithPermit::Balance {} => {
            if !permit.check_permission(&TokenPermissions::Balance) {
                return Err(StdError::generic_err(format!(
                    "No permission to query balance, got permissions {:?}",
                    permit.params.permissions
                )));
            }

            query_balance(deps, &account)
        }
        QueryWithPermit::TransferHistory { page, page_size } => {
            if !permit.check_permission(&TokenPermissions::History) {
                return Err(StdError::generic_err(format!(
                    "No permission to query history, got permissions {:?}",
                    permit.params.permissions
                )));
            }

            query_transfers(deps, &account, page.unwrap_or(0), page_size)
        }
        QueryWithPermit::TransactionHistory { page, page_size } => {
            if !permit.check_permission(&TokenPermissions::History) {
                return Err(StdError::generic_err(format!(
                    "No permission to query history, got permissions {:?}",
                    permit.params.permissions
                )));
            }

            query_transactions(deps, &account, page.unwrap_or(0), page_size)
        }
        QueryWithPermit::Allowance { owner, spender } => {
            if !permit.check_permission(&TokenPermissions::Allowance) {
                return Err(StdError::generic_err(format!(
                    "No permission to query allowance, got permissions {:?}",
                    permit.params.permissions
                )));
            }

            if account != owner && account != spender {
                return Err(StdError::generic_err(format!(
                    "Cannot query allowance. Requires permit for either owner {:?} or spender {:?}, got permit for {:?}",
                    owner.as_str(), spender.as_str(), account.as_str()
                )));
            }

            query_allowance(deps, owner, spender)
        }
    }
}

fn is_admin<S: Storage>(config: &Config<S>, account: &HumanAddr) -> StdResult<bool> {
    let consts = config.constants()?;
    if &consts.admin != account {
        return Ok(false);
    }

    Ok(true)
}

pub fn check_if_admin<S: Storage>(config: &Config<S>, account: &HumanAddr) -> StdResult<()> {
    if !is_admin(config, account)? {
        return Err(StdError::generic_err(
            "This is an admin command. Admin commands can only be run from admin address",
        ));
    }

    Ok(())
}

fn is_valid_name(name: &str) -> bool {
    let len = name.len();
    (3..=30).contains(&len)
}

fn is_valid_symbol(symbol: &str) -> bool {
    let len = symbol.len();
    let len_is_valid = (3..=6).contains(&len);

    len_is_valid && symbol.bytes().all(|byte| (b'A'..=b'Z').contains(&byte))
}

fn pad_response(response: StdResult<HandleResponse>) -> StdResult<HandleResponse> {
    response.map(|mut response| {
        response.data = response.data.map(|mut data| {
            space_pad(RESPONSE_BLOCK_SIZE, &mut data.0);
            data
        });
        response
    })
}




