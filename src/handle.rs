
use cosmwasm_std::{Api, BankMsg, Binary, CanonicalAddr, Coin, CosmosMsg, Env, Extern, HandleResponse, HumanAddr, log, Querier, ReadonlyStorage, StdError, StdResult, Storage, to_binary, Uint128};
use hermit_toolkit::permit::RevokedPermits;
use crate::batch;
use crate::contract::{check_if_admin, PREFIX_REVOKED_PERMITS};
use crate::msg::{ContractStatusLevel, HandleAnswer};
use crate::msg::ResponseStatus::Success;
use crate::receiver::Hmip20ReceiveMsg;
use crate::state::{Balances, Config, get_receiver_hash, read_allowance, ReadonlyConfig, set_receiver_hash, write_allowance, write_viewing_key};
use crate::tools::viewing_key::ViewingKey;
use crate::transaction_history::{store_burn, store_deposit, store_mint, store_redeem, store_transfer};



pub fn try_deposit<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
) -> StdResult<HandleResponse> {
    let mut amount = Uint128::zero();

    for coin in &env.message.sent_funds {
        if coin.denom == "ughm" {
            amount = coin.amount
        } else {
            return Err(StdError::generic_err(
                "Tried to deposit an unsupported token",
            ));
        }
    }

    if amount.is_zero() {
        return Err(StdError::generic_err("No funds were sent to be deposited"));
    }

    let raw_amount = amount.u128();

    let mut config = Config::from_storage(&mut deps.storage);
    let constants = config.constants()?;
    if !constants.deposit_is_enabled {
        return Err(StdError::generic_err(
            "Deposit functionality is not enabled for this token.",
        ));
    }
    let total_supply = config.total_supply();
    if let Some(total_supply) = total_supply.checked_add(raw_amount) {
        config.set_total_supply(total_supply);
    } else {
        return Err(StdError::generic_err(
            "This deposit would overflow the currency's total supply",
        ));
    }

    let sender_address = deps.api.canonical_address(&env.message.sender)?;

    let mut balances = Balances::from_storage(&mut deps.storage);
    let account_balance = balances.balance(&sender_address);
    if let Some(account_balance) = account_balance.checked_add(raw_amount) {
        balances.set_account_balance(&sender_address, account_balance);
    } else {
        return Err(StdError::generic_err(
            "This deposit would overflow your balance",
        ));
    }

    store_deposit(
        &mut deps.storage,
        &sender_address,
        amount,
        "ughm".to_string(),
        &env.block,
    )?;

    let res = HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::Deposit { status: Success })?),
    };

    Ok(res)
}

pub fn try_redeem<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    amount: Uint128,
) -> StdResult<HandleResponse> {
    let config = ReadonlyConfig::from_storage(&deps.storage);
    let constants = config.constants()?;
    if !constants.redeem_is_enabled {
        return Err(StdError::generic_err(
            "Redeem functionality is not enabled for this token.",
        ));
    }

    let sender_address = deps.api.canonical_address(&env.message.sender)?;
    let amount_raw = amount.u128();

    let mut balances = Balances::from_storage(&mut deps.storage);
    let account_balance = balances.balance(&sender_address);

    if let Some(account_balance) = account_balance.checked_sub(amount_raw) {
        balances.set_account_balance(&sender_address, account_balance);
    } else {
        return Err(StdError::generic_err(format!(
            "insufficient funds to redeem: balance={}, required={}",
            account_balance, amount_raw
        )));
    }

    let mut config = Config::from_storage(&mut deps.storage);
    let total_supply = config.total_supply();
    if let Some(total_supply) = total_supply.checked_sub(amount_raw) {
        config.set_total_supply(total_supply);
    } else {
        return Err(StdError::generic_err(
            "You are trying to redeem more tokens than what is available in the total supply",
        ));
    }

    let token_reserve = deps
        .querier
        .query_balance(&env.contract.address, "ughm")?
        .amount;
    if amount > token_reserve {
        return Err(StdError::generic_err(
            "You are trying to redeem for more SCRT than the token has in its deposit reserve.",
        ));
    }

    let withdrawal_coins: Vec<Coin> = vec![Coin {
        denom: "ughm".to_string(),
        amount,
    }];

    store_redeem(
        &mut deps.storage,
        &sender_address,
        amount,
        constants.symbol,
        &env.block,
    )?;

    let res = HandleResponse {
        messages: vec![CosmosMsg::Bank(BankMsg::Send {
            from_address: env.contract.address,
            to_address: env.message.sender,
            amount: withdrawal_coins,
        })],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::Redeem { status: Success })?),
    };

    Ok(res)
}

pub fn try_transfer_impl<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    sender: &CanonicalAddr,
    recipient: &CanonicalAddr,
    amount: Uint128,
    memo: Option<String>,
    block: &cosmwasm_std::BlockInfo,
) -> StdResult<()> {
    perform_transfer(&mut deps.storage, sender, recipient, amount.u128())?;

    let symbol = Config::from_storage(&mut deps.storage).constants()?.symbol;

    store_transfer(
        &mut deps.storage,
        sender,
        sender,
        recipient,
        amount,
        symbol,
        memo,
        block,
    )?;

    Ok(())
}

pub fn try_transfer<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    recipient: HumanAddr,
    amount: Uint128,
    memo: Option<String>,
) -> StdResult<HandleResponse> {
    let sender = deps.api.canonical_address(&env.message.sender)?;
    let recipient = deps.api.canonical_address(&recipient)?;
    try_transfer_impl(deps, &sender, &recipient, amount, memo, &env.block)?;

    let res = HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::Transfer { status: Success })?),
    };
    Ok(res)
}

pub fn try_batch_transfer<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    actions: Vec<batch::TransferAction>,
) -> StdResult<HandleResponse> {
    let sender = deps.api.canonical_address(&env.message.sender)?;
    for action in actions {
        let recipient = deps.api.canonical_address(&action.recipient)?;
        try_transfer_impl(
            deps,
            &sender,
            &recipient,
            action.amount,
            action.memo,
            &env.block,
        )?;
    }

    let res = HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::BatchTransfer { status: Success })?),
    };
    Ok(res)
}

#[allow(clippy::too_many_arguments)]
pub fn try_add_receiver_api_callback<S: ReadonlyStorage>(
    storage: &S,
    messages: &mut Vec<CosmosMsg>,
    recipient: HumanAddr,
    recipient_code_hash: Option<String>,
    msg: Option<Binary>,
    sender: HumanAddr,
    from: HumanAddr,
    amount: Uint128,
    memo: Option<String>,
) -> StdResult<()> {
    if let Some(receiver_hash) = recipient_code_hash {
        let receiver_msg = Hmip20ReceiveMsg::new(sender, from, amount, memo, msg);
        let callback_msg = receiver_msg.into_cosmos_msg(receiver_hash, recipient)?;

        messages.push(callback_msg);
        return Ok(());
    }

    let receiver_hash = get_receiver_hash(storage, &recipient);
    if let Some(receiver_hash) = receiver_hash {
        let receiver_hash = receiver_hash?;
        let receiver_msg = Hmip20ReceiveMsg::new(sender, from, amount, memo, msg);
        let callback_msg = receiver_msg.into_cosmos_msg(receiver_hash, recipient)?;

        messages.push(callback_msg);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn try_send_impl<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    messages: &mut Vec<CosmosMsg>,
    sender: HumanAddr,
    sender_canon: &CanonicalAddr, // redundant but more efficient
    recipient: HumanAddr,
    recipient_code_hash: Option<String>,
    amount: Uint128,
    memo: Option<String>,
    msg: Option<Binary>,
    block: &cosmwasm_std::BlockInfo,
) -> StdResult<()> {
    let recipient_canon = deps.api.canonical_address(&recipient)?;
    try_transfer_impl(
        deps,
        sender_canon,
        &recipient_canon,
        amount,
        memo.clone(),
        block,
    )?;

    try_add_receiver_api_callback(
        &deps.storage,
        messages,
        recipient,
        recipient_code_hash,
        msg,
        sender.clone(),
        sender,
        amount,
        memo,
    )?;

    Ok(())
}

pub fn try_send<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    recipient: HumanAddr,
    recipient_code_hash: Option<String>,
    amount: Uint128,
    memo: Option<String>,
    msg: Option<Binary>,
) -> StdResult<HandleResponse> {
    let mut messages = vec![];
    let sender = env.message.sender;
    let sender_canon = deps.api.canonical_address(&sender)?;
    try_send_impl(
        deps,
        &mut messages,
        sender,
        &sender_canon,
        recipient,
        recipient_code_hash,
        amount,
        memo,
        msg,
        &env.block,
    )?;

    let res = HandleResponse {
        messages,
        log: vec![],
        data: Some(to_binary(&HandleAnswer::Send { status: Success })?),
    };
    Ok(res)
}

pub fn try_batch_send<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    actions: Vec<batch::SendAction>,
) -> StdResult<HandleResponse> {
    let mut messages = vec![];
    let sender = env.message.sender;
    let sender_canon = deps.api.canonical_address(&sender)?;
    for action in actions {
        try_send_impl(
            deps,
            &mut messages,
            sender.clone(),
            &sender_canon,
            action.recipient,
            action.recipient_code_hash,
            action.amount,
            action.memo,
            action.msg,
            &env.block,
        )?;
    }

    let res = HandleResponse {
        messages,
        log: vec![],
        data: Some(to_binary(&HandleAnswer::BatchSend { status: Success })?),
    };
    Ok(res)
}

pub fn try_register_receive<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    code_hash: String,
) -> StdResult<HandleResponse> {
    set_receiver_hash(&mut deps.storage, &env.message.sender, code_hash);
    let res = HandleResponse {
        messages: vec![],
        log: vec![log("register_status", "success")],
        data: Some(to_binary(&HandleAnswer::RegisterReceive {
            status: Success,
        })?),
    };
    Ok(res)
}

pub fn insufficient_allowance(allowance: u128, required: u128) -> StdError {
    StdError::generic_err(format!(
        "insufficient allowance: allowance={}, required={}",
        allowance, required
    ))
}

pub fn use_allowance<S: Storage>(
    storage: &mut S,
    env: &Env,
    owner: &CanonicalAddr,
    spender: &CanonicalAddr,
    amount: u128,
) -> StdResult<()> {
    let mut allowance = read_allowance(storage, owner, spender)?;

    if allowance.is_expired_at(&env.block) {
        return Err(insufficient_allowance(0, amount));
    }
    if let Some(new_allowance) = allowance.amount.checked_sub(amount) {
        allowance.amount = new_allowance;
    } else {
        return Err(insufficient_allowance(allowance.amount, amount));
    }

    write_allowance(storage, owner, spender, allowance)?;

    Ok(())
}

pub fn try_transfer_from_impl<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: &Env,
    spender: &CanonicalAddr,
    owner: &CanonicalAddr,
    recipient: &CanonicalAddr,
    amount: Uint128,
    memo: Option<String>,
) -> StdResult<()> {
    let raw_amount = amount.u128();

    use_allowance(&mut deps.storage, env, owner, spender, raw_amount)?;

    perform_transfer(&mut deps.storage, owner, recipient, raw_amount)?;

    let symbol = Config::from_storage(&mut deps.storage).constants()?.symbol;

    store_transfer(
        &mut deps.storage,
        owner,
        spender,
        recipient,
        amount,
        symbol,
        memo,
        &env.block,
    )?;

    Ok(())
}

pub fn try_transfer_from<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: &Env,
    owner: &HumanAddr,
    recipient: &HumanAddr,
    amount: Uint128,
    memo: Option<String>,
) -> StdResult<HandleResponse> {
    let spender = deps.api.canonical_address(&env.message.sender)?;
    let owner = deps.api.canonical_address(owner)?;
    let recipient = deps.api.canonical_address(recipient)?;
    try_transfer_from_impl(deps, env, &spender, &owner, &recipient, amount, memo)?;

    let res = HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::TransferFrom { status: Success })?),
    };
    Ok(res)
}

pub fn try_batch_transfer_from<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: &Env,
    actions: Vec<batch::TransferFromAction>,
) -> StdResult<HandleResponse> {
    let spender = deps.api.canonical_address(&env.message.sender)?;
    for action in actions {
        let owner = deps.api.canonical_address(&action.owner)?;
        let recipient = deps.api.canonical_address(&action.recipient)?;
        try_transfer_from_impl(
            deps,
            env,
            &spender,
            &owner,
            &recipient,
            action.amount,
            action.memo,
        )?;
    }

    let res = HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::BatchTransferFrom {
            status: Success,
        })?),
    };
    Ok(res)
}

#[allow(clippy::too_many_arguments)]
pub fn try_send_from_impl<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    messages: &mut Vec<CosmosMsg>,
    spender_canon: &CanonicalAddr, // redundant but more efficient
    owner: HumanAddr,
    recipient: HumanAddr,
    recipient_code_hash: Option<String>,
    amount: Uint128,
    memo: Option<String>,
    msg: Option<Binary>,
) -> StdResult<()> {
    let owner_canon = deps.api.canonical_address(&owner)?;
    let recipient_canon = deps.api.canonical_address(&recipient)?;
    try_transfer_from_impl(
        deps,
        &env,
        spender_canon,
        &owner_canon,
        &recipient_canon,
        amount,
        memo.clone(),
    )?;

    try_add_receiver_api_callback(
        &deps.storage,
        messages,
        recipient,
        recipient_code_hash,
        msg,
        env.message.sender,
        owner,
        amount,
        memo,
    )?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn try_send_from<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    owner: HumanAddr,
    recipient: HumanAddr,
    recipient_code_hash: Option<String>,
    amount: Uint128,
    memo: Option<String>,
    msg: Option<Binary>,
) -> StdResult<HandleResponse> {
    let spender = &env.message.sender;
    let spender_canon = deps.api.canonical_address(spender)?;

    let mut messages = vec![];
    try_send_from_impl(
        deps,
        env,
        &mut messages,
        &spender_canon,
        owner,
        recipient,
        recipient_code_hash,
        amount,
        memo,
        msg,
    )?;

    let res = HandleResponse {
        messages,
        log: vec![],
        data: Some(to_binary(&HandleAnswer::SendFrom { status: Success })?),
    };
    Ok(res)
}

pub fn try_batch_send_from<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    actions: Vec<batch::SendFromAction>,
) -> StdResult<HandleResponse> {
    let spender = &env.message.sender;
    let spender_canon = deps.api.canonical_address(spender)?;
    let mut messages = vec![];

    for action in actions {
        try_send_from_impl(
            deps,
            env.clone(),
            &mut messages,
            &spender_canon,
            action.owner,
            action.recipient,
            action.recipient_code_hash,
            action.amount,
            action.memo,
            action.msg,
        )?;
    }

    let res = HandleResponse {
        messages,
        log: vec![],
        data: Some(to_binary(&HandleAnswer::BatchSendFrom { status: Success })?),
    };
    Ok(res)
}

pub fn try_burn_from<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: &Env,
    owner: &HumanAddr,
    amount: Uint128,
    memo: Option<String>,
) -> StdResult<HandleResponse> {
    let config = ReadonlyConfig::from_storage(&deps.storage);
    let constants = config.constants()?;
    if !constants.burn_is_enabled {
        return Err(StdError::generic_err(
            "Burn functionality is not enabled for this token.",
        ));
    }

    let spender = deps.api.canonical_address(&env.message.sender)?;
    let owner = deps.api.canonical_address(owner)?;
    let raw_amount = amount.u128();
    use_allowance(&mut deps.storage, env, &owner, &spender, raw_amount)?;

    // subtract from owner account
    let mut balances = Balances::from_storage(&mut deps.storage);
    let mut account_balance = balances.balance(&owner);

    if let Some(new_balance) = account_balance.checked_sub(raw_amount) {
        account_balance = new_balance;
    } else {
        return Err(StdError::generic_err(format!(
            "insufficient funds to burn: balance={}, required={}",
            account_balance, raw_amount
        )));
    }
    balances.set_account_balance(&owner, account_balance);

    // remove from supply
    let mut config = Config::from_storage(&mut deps.storage);
    let mut total_supply = config.total_supply();
    if let Some(new_total_supply) = total_supply.checked_sub(raw_amount) {
        total_supply = new_total_supply;
    } else {
        return Err(StdError::generic_err(
            "You're trying to burn more than is available in the total supply",
        ));
    }
    config.set_total_supply(total_supply);

    store_burn(
        &mut deps.storage,
        &owner,
        &spender,
        amount,
        constants.symbol,
        memo,
        &env.block,
    )?;

    let res = HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::BurnFrom { status: Success })?),
    };

    Ok(res)
}

pub fn try_batch_burn_from<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: &Env,
    actions: Vec<batch::BurnFromAction>,
) -> StdResult<HandleResponse> {
    let config = ReadonlyConfig::from_storage(&deps.storage);
    let constants = config.constants()?;
    if !constants.burn_is_enabled {
        return Err(StdError::generic_err(
            "Burn functionality is not enabled for this token.",
        ));
    }

    let spender = deps.api.canonical_address(&env.message.sender)?;

    let mut total_supply = config.total_supply();

    for action in actions {
        let owner = deps.api.canonical_address(&action.owner)?;
        let amount = action.amount.u128();
        use_allowance(&mut deps.storage, env, &owner, &spender, amount)?;

        // subtract from owner account
        let mut balances = Balances::from_storage(&mut deps.storage);
        let mut account_balance = balances.balance(&owner);

        if let Some(new_balance) = account_balance.checked_sub(amount) {
            account_balance = new_balance;
        } else {
            return Err(StdError::generic_err(format!(
                "insufficient funds to burn: balance={}, required={}",
                account_balance, amount
            )));
        }
        balances.set_account_balance(&owner, account_balance);

        // remove from supply
        if let Some(new_total_supply) = total_supply.checked_sub(amount) {
            total_supply = new_total_supply;
        } else {
            return Err(StdError::generic_err(format!(
                "You're trying to burn more than is available in the total supply: {:?}",
                action
            )));
        }

        store_burn(
            &mut deps.storage,
            &owner,
            &spender,
            action.amount,
            constants.symbol.clone(),
            action.memo,
            &env.block,
        )?;
    }

    let mut config = Config::from_storage(&mut deps.storage);
    config.set_total_supply(total_supply);

    let res = HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::BatchBurnFrom { status: Success })?),
    };

    Ok(res)
}

pub fn try_increase_allowance<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    spender: HumanAddr,
    amount: Uint128,
    expiration: Option<u64>,
) -> StdResult<HandleResponse> {
    let owner_address = deps.api.canonical_address(&env.message.sender)?;
    let spender_address = deps.api.canonical_address(&spender)?;

    let mut allowance = read_allowance(&deps.storage, &owner_address, &spender_address)?;

    // If the previous allowance has expired, reset the allowance.
    // Without this users can take advantage of an expired allowance given to
    // them long ago.
    if allowance.is_expired_at(&env.block) {
        allowance.amount = amount.u128();
        allowance.expiration = None;
    } else {
        allowance.amount = allowance.amount.saturating_add(amount.u128());
    }

    if expiration.is_some() {
        allowance.expiration = expiration;
    }
    let new_amount = allowance.amount;
    write_allowance(
        &mut deps.storage,
        &owner_address,
        &spender_address,
        allowance,
    )?;

    let res = HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::IncreaseAllowance {
            owner: env.message.sender,
            spender,
            allowance: Uint128(new_amount),
        })?),
    };
    Ok(res)
}

pub fn try_decrease_allowance<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    spender: HumanAddr,
    amount: Uint128,
    expiration: Option<u64>,
) -> StdResult<HandleResponse> {
    let owner_address = deps.api.canonical_address(&env.message.sender)?;
    let spender_address = deps.api.canonical_address(&spender)?;

    let mut allowance = read_allowance(&deps.storage, &owner_address, &spender_address)?;

    // If the previous allowance has expired, reset the allowance.
    // Without this users can take advantage of an expired allowance given to
    // them long ago.
    if allowance.is_expired_at(&env.block) {
        allowance.amount = 0;
        allowance.expiration = None;
    } else {
        allowance.amount = allowance.amount.saturating_sub(amount.u128());
    }

    if expiration.is_some() {
        allowance.expiration = expiration;
    }
    let new_amount = allowance.amount;
    write_allowance(
        &mut deps.storage,
        &owner_address,
        &spender_address,
        allowance,
    )?;

    let res = HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::DecreaseAllowance {
            owner: env.message.sender,
            spender,
            allowance: Uint128(new_amount),
        })?),
    };
    Ok(res)
}

pub fn add_minters<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    minters_to_add: Vec<HumanAddr>,
) -> StdResult<HandleResponse> {
    let mut config = Config::from_storage(&mut deps.storage);
    let constants = config.constants()?;
    if !constants.mint_is_enabled {
        return Err(StdError::generic_err(
            "Mint functionality is not enabled for this token.",
        ));
    }

    check_if_admin(&config, &env.message.sender)?;

    config.add_minters(minters_to_add)?;

    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::AddMinters { status: Success })?),
    })
}

pub fn remove_minters<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    minters_to_remove: Vec<HumanAddr>,
) -> StdResult<HandleResponse> {
    let mut config = Config::from_storage(&mut deps.storage);
    let constants = config.constants()?;
    if !constants.mint_is_enabled {
        return Err(StdError::generic_err(
            "Mint functionality is not enabled for this token.",
        ));
    }

    check_if_admin(&config, &env.message.sender)?;

    config.remove_minters(minters_to_remove)?;

    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::RemoveMinters { status: Success })?),
    })
}

pub fn set_minters<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    minters_to_set: Vec<HumanAddr>,
) -> StdResult<HandleResponse> {
    let mut config = Config::from_storage(&mut deps.storage);
    let constants = config.constants()?;
    if !constants.mint_is_enabled {
        return Err(StdError::generic_err(
            "Mint functionality is not enabled for this token.",
        ));
    }

    check_if_admin(&config, &env.message.sender)?;

    config.set_minters(minters_to_set)?;

    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::SetMinters { status: Success })?),
    })
}

/// Burn tokens
///
/// Remove `amount` tokens from the system irreversibly, from signer account
///
/// @param amount the amount of money to burn
pub fn try_burn<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    amount: Uint128,
    memo: Option<String>,
) -> StdResult<HandleResponse> {
    let config = ReadonlyConfig::from_storage(&deps.storage);
    let constants = config.constants()?;
    if !constants.burn_is_enabled {
        return Err(StdError::generic_err(
            "Burn functionality is not enabled for this token.",
        ));
    }

    let sender_address = deps.api.canonical_address(&env.message.sender)?;
    let raw_amount = amount.u128();

    let mut balances = Balances::from_storage(&mut deps.storage);
    let mut account_balance = balances.balance(&sender_address);

    if let Some(new_account_balance) = account_balance.checked_sub(raw_amount) {
        account_balance = new_account_balance;
    } else {
        return Err(StdError::generic_err(format!(
            "insufficient funds to burn: balance={}, required={}",
            account_balance, raw_amount
        )));
    }

    balances.set_account_balance(&sender_address, account_balance);

    let mut config = Config::from_storage(&mut deps.storage);
    let mut total_supply = config.total_supply();
    if let Some(new_total_supply) = total_supply.checked_sub(raw_amount) {
        total_supply = new_total_supply;
    } else {
        return Err(StdError::generic_err(
            "You're trying to burn more than is available in the total supply",
        ));
    }
    config.set_total_supply(total_supply);

    store_burn(
        &mut deps.storage,
        &sender_address,
        &sender_address,
        amount,
        constants.symbol,
        memo,
        &env.block,
    )?;

    let res = HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::Burn { status: Success })?),
    };

    Ok(res)
}

pub fn perform_transfer<T: Storage>(
    store: &mut T,
    from: &CanonicalAddr,
    to: &CanonicalAddr,
    amount: u128,
) -> StdResult<()> {
    let mut balances = Balances::from_storage(store);

    let mut from_balance = balances.balance(from);
    if let Some(new_from_balance) = from_balance.checked_sub(amount) {
        from_balance = new_from_balance;
    } else {
        return Err(StdError::generic_err(format!(
            "insufficient funds: balance={}, required={}",
            from_balance, amount
        )));
    }
    balances.set_account_balance(from, from_balance);

    let mut to_balance = balances.balance(to);
    to_balance = to_balance.checked_add(amount).ok_or_else(|| {
        StdError::generic_err("This tx will literally make them too rich. Try transferring less")
    })?;
    balances.set_account_balance(to, to_balance);

    Ok(())
}

pub fn revoke_permit<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    permit_name: String,
) -> StdResult<HandleResponse> {
    RevokedPermits::revoke_permit(
        &mut deps.storage,
        PREFIX_REVOKED_PERMITS,
        &env.message.sender,
        &permit_name,
    );

    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::RevokePermit { status: Success })?),
    })
}

pub fn change_admin<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    address: HumanAddr,
) -> StdResult<HandleResponse> {
    let mut config = Config::from_storage(&mut deps.storage);

    check_if_admin(&config, &env.message.sender)?;

    let mut consts = config.constants()?;
    consts.admin = address;
    config.set_constants(&consts)?;

    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::ChangeAdmin { status: Success })?),
    })
}

pub fn try_mint_impl<S: Storage>(
    storage: &mut S,
    minter: &CanonicalAddr,
    recipient: &CanonicalAddr,
    amount: Uint128,
    denom: String,
    memo: Option<String>,
    block: &cosmwasm_std::BlockInfo,
) -> StdResult<()> {
    let raw_amount = amount.u128();

    let mut balances = Balances::from_storage(storage);

    let mut account_balance = balances.balance(recipient);

    if let Some(new_balance) = account_balance.checked_add(raw_amount) {
        account_balance = new_balance;
    } else {
        // This error literally can not happen, since the account's funds are a subset
        // of the total supply, both are stored as u128, and we check for overflow of
        // the total supply just a couple lines before.
        // Still, writing this to cover all overflows.
        return Err(StdError::generic_err(
            "This mint attempt would increase the account's balance above the supported maximum",
        ));
    }

    balances.set_account_balance(recipient, account_balance);

    store_mint(storage, minter, recipient, amount, denom, memo, block)?;

    Ok(())
}

pub fn try_mint<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    recipient: HumanAddr,
    amount: Uint128,
    memo: Option<String>,
) -> StdResult<HandleResponse> {
    let mut config = Config::from_storage(&mut deps.storage);
    let constants = config.constants()?;
    if !constants.mint_is_enabled {
        return Err(StdError::generic_err(
            "Mint functionality is not enabled for this token.",
        ));
    }

    let minters = config.minters();
    if !minters.contains(&env.message.sender) {
        return Err(StdError::generic_err(
            "Minting is allowed to minter accounts only",
        ));
    }

    let mut total_supply = config.total_supply();
    if let Some(new_total_supply) = total_supply.checked_add(amount.u128()) {
        total_supply = new_total_supply;
    } else {
        return Err(StdError::generic_err(
            "This mint attempt would increase the total supply above the supported maximum",
        ));
    }
    config.set_total_supply(total_supply);

    let minter = deps.api.canonical_address(&env.message.sender)?;
    let recipient = deps.api.canonical_address(&recipient)?;
    try_mint_impl(
        &mut deps.storage,
        &minter,
        &recipient,
        amount,
        constants.symbol,
        memo,
        &env.block,
    )?;

    let res = HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::Mint { status: Success })?),
    };

    Ok(res)
}

pub fn try_batch_mint<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    actions: Vec<batch::MintAction>,
) -> StdResult<HandleResponse> {
    let mut config = Config::from_storage(&mut deps.storage);
    let constants = config.constants()?;
    if !constants.mint_is_enabled {
        return Err(StdError::generic_err(
            "Mint functionality is not enabled for this token.",
        ));
    }

    let minters = config.minters();
    if !minters.contains(&env.message.sender) {
        return Err(StdError::generic_err(
            "Minting is allowed to minter accounts only",
        ));
    }

    let mut total_supply = config.total_supply();

    // Quick loop to check that the total of amounts is valid
    for action in &actions {
        if let Some(new_total_supply) = total_supply.checked_add(action.amount.u128()) {
            total_supply = new_total_supply;
        } else {
            return Err(StdError::generic_err(
                format!("This mint attempt would increase the total supply above the supported maximum: {:?}", action),
            ));
        }
    }
    config.set_total_supply(total_supply);

    let minter = deps.api.canonical_address(&env.message.sender)?;
    for action in actions {
        let recipient = deps.api.canonical_address(&action.recipient)?;
        try_mint_impl(
            &mut deps.storage,
            &minter,
            &recipient,
            action.amount,
            constants.symbol.clone(),
            action.memo,
            &env.block,
        )?;
    }

    let res = HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::BatchMint { status: Success })?),
    };

    Ok(res)
}

pub fn try_set_key<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    key: String,
) -> StdResult<HandleResponse> {
    let vk = ViewingKey(key);

    let message_sender = deps.api.canonical_address(&env.message.sender)?;
    write_viewing_key(&mut deps.storage, &message_sender, &vk);

    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::SetViewingKey { status: Success })?),
    })
}

pub fn try_create_key<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    entropy: String,
) -> StdResult<HandleResponse> {
    let constants = ReadonlyConfig::from_storage(&deps.storage).constants()?;
    let prng_seed = constants.prng_seed;

    let key = ViewingKey::new(&env, &prng_seed, (&entropy).as_ref());

    let message_sender = deps.api.canonical_address(&env.message.sender)?;
    write_viewing_key(&mut deps.storage, &message_sender, &key);

    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::CreateViewingKey { key })?),
    })
}

pub fn set_contract_status<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    status_level: ContractStatusLevel,
) -> StdResult<HandleResponse> {
    let mut config = Config::from_storage(&mut deps.storage);

    check_if_admin(&config, &env.message.sender)?;

    config.set_contract_status(status_level);

    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::SetContractStatus {
            status: Success,
        })?),
    })
}

