use {
    anchor_lang::{solana_program::program_pack::Pack, AccountDeserialize},
    anchor_spl::associated_token,
    anchor_spl::token::spl_token,
    litesvm::LiteSVM,
    litesvm_token::CreateMint,
    solana_keypair::Keypair,
    solana_message::{Instruction, Message, VersionedMessage},
    solana_pubkey::Pubkey,
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
};

mod ix_handlers;
use ix_handlers::*;

fn send(
    svm: &mut LiteSVM,
    ixs: &[Instruction],
    payer: &Keypair,
    signers: &[&Keypair],
) -> litesvm::types::TransactionResult {
    svm.expire_blockhash();
    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(ixs, Some(&payer.pubkey()), &blockhash);
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), signers).unwrap();
    svm.send_transaction(tx)
}

fn token_balance(svm: &LiteSVM, address: &Pubkey) -> u64 {
    spl_token::state::Account::unpack(&svm.get_account(address).unwrap().data)
        .unwrap()
        .amount
}

fn mint_supply(svm: &LiteSVM, address: &Pubkey) -> u64 {
    spl_token::state::Mint::unpack(&svm.get_account(address).unwrap().data)
        .unwrap()
        .supply
}

// Setup function to initialize LiteSVM and create a payer keypair
fn setup() -> (
    LiteSVM,
    Keypair,
    Pubkey,
    Pubkey,
    Pubkey,
    Pubkey,
    Pubkey,
    Pubkey,
    Pubkey,
    Pubkey,
    Pubkey,
) {
    let program_id = amm_video::id();
    let payer = Keypair::new();
    let mut svm = LiteSVM::new();
    let bytes = include_bytes!("../../../target/deploy/amm_video.so");
    svm.add_program(program_id, bytes).unwrap();
    svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();

    // Create two mints (Mint A and Mint B) with 6 decimal places and the maker as the authority
    // This done using litesvm-token's CreateMint utility which creates the mint in the LiteSVM environment
    let mint_x = CreateMint::new(&mut svm, &payer)
        .decimals(6)
        .authority(&payer.pubkey())
        .send()
        .unwrap();

    let mint_y = CreateMint::new(&mut svm, &payer)
        .decimals(6)
        .authority(&payer.pubkey())
        .send()
        .unwrap();

    let config =
        Pubkey::find_program_address(&[b"config", &123u64.to_le_bytes()], &amm_video::id()).0;
    let mint_lp = Pubkey::find_program_address(&[b"lp", config.as_ref()], &amm_video::id()).0;

    // Derive the PDA for the vault associated token account using the config PDA and Mint A
    let vault_x = associated_token::get_associated_token_address(&config, &mint_x);
    let vault_y = associated_token::get_associated_token_address(&config, &mint_y);
    let treasury =
        Pubkey::find_program_address(&[b"treasury", config.as_ref()], &amm_video::id()).0;
    let treasury_x = associated_token::get_associated_token_address(&treasury, &mint_x);
    let treasury_y = associated_token::get_associated_token_address(&treasury, &mint_y);

    (
        svm, payer, mint_x, mint_y, config, mint_lp, vault_x, vault_y, treasury, treasury_x,
        treasury_y,
    )
}

#[test]
fn test_initialize() {
    let (
        mut svm,
        payer,
        mint_x,
        mint_y,
        config,
        mint_lp,
        vault_x,
        vault_y,
        treasury,
        treasury_x,
        treasury_y,
    ) = setup();

    let instruction = create_initialise_ix(
        &mut svm, &payer, mint_x, mint_y, config, mint_lp, vault_x, vault_y, treasury, treasury_x,
        treasury_y,
    );
    let res = send(&mut svm, &[instruction], &payer, &[&payer]);
    assert!(res.is_ok());

    let config_data =
        amm_video::Config::try_deserialize(&mut svm.get_account(&config).unwrap().data.as_ref())
            .unwrap();
    assert_eq!(config_data.mint_x, mint_x);
    assert_eq!(config_data.mint_y, mint_y);
    assert_eq!(config_data.fee, 30);
    assert!(!config_data.locked);
    assert_eq!(mint_supply(&svm, &mint_lp), 0);
    assert_eq!(token_balance(&svm, &vault_x), 0);
    assert_eq!(token_balance(&svm, &vault_y), 0);
    assert_eq!(token_balance(&svm, &treasury_x), 0);
    assert_eq!(token_balance(&svm, &treasury_y), 0);
}

#[test]
pub fn test_deposit() {
    let (
        mut svm,
        payer,
        mint_x,
        mint_y,
        config,
        mint_lp,
        vault_x,
        vault_y,
        treasury,
        treasury_x,
        treasury_y,
    ) = setup();
    let init_ix = create_initialise_ix(
        &mut svm, &payer, mint_x, mint_y, config, mint_lp, vault_x, vault_y, treasury, treasury_x,
        treasury_y,
    );

    let deposit_ix = create_deposit_ix(
        &mut svm, &payer, mint_x, mint_y, mint_lp, config, vault_x, vault_y,
    );

    let res = send(&mut svm, &[init_ix, deposit_ix], &payer, &[&payer]);
    assert!(res.is_ok());

    let user_x = associated_token::get_associated_token_address(&payer.pubkey(), &mint_x);
    let user_y = associated_token::get_associated_token_address(&payer.pubkey(), &mint_y);
    let user_lp = associated_token::get_associated_token_address(&payer.pubkey(), &mint_lp);
    assert_eq!(token_balance(&svm, &vault_x), 200_000_000);
    assert_eq!(token_balance(&svm, &vault_y), 200_000_000);
    assert_eq!(token_balance(&svm, &user_x), 800_000_000);
    assert_eq!(token_balance(&svm, &user_y), 800_000_000);
    assert_eq!(token_balance(&svm, &user_lp), 100_000_000);
    assert_eq!(mint_supply(&svm, &mint_lp), 100_000_000);
}

#[test]
pub fn test_withdraw() {
    let (
        mut svm,
        payer,
        mint_x,
        mint_y,
        config,
        mint_lp,
        vault_x,
        vault_y,
        treasury,
        treasury_x,
        treasury_y,
    ) = setup();
    let init_ix = create_initialise_ix(
        &mut svm, &payer, mint_x, mint_y, config, mint_lp, vault_x, vault_y, treasury, treasury_x,
        treasury_y,
    );

    let deposit_ix = create_deposit_ix(
        &mut svm, &payer, mint_x, mint_y, mint_lp, config, vault_x, vault_y,
    );

    let withdraw_ix = create_withdraw_ix(
        &mut svm, &payer, mint_x, mint_y, mint_lp, config, vault_x, vault_y,
    );
    let res = send(
        &mut svm,
        &[init_ix, deposit_ix, withdraw_ix],
        &payer,
        &[&payer],
    );
    assert!(res.is_ok());

    let user_x = associated_token::get_associated_token_address(&payer.pubkey(), &mint_x);
    let user_y = associated_token::get_associated_token_address(&payer.pubkey(), &mint_y);
    let user_lp = associated_token::get_associated_token_address(&payer.pubkey(), &mint_lp);
    assert_eq!(token_balance(&svm, &vault_x), 166_666_666);
    assert_eq!(token_balance(&svm, &vault_y), 166_666_666);
    assert_eq!(token_balance(&svm, &user_x), 833_333_334);
    assert_eq!(token_balance(&svm, &user_y), 833_333_334);
    assert_eq!(token_balance(&svm, &user_lp), 90_000_000);
    assert_eq!(mint_supply(&svm, &mint_lp), 90_000_000);
}

#[test]
pub fn test_swap() {
    let (
        mut svm,
        payer,
        mint_x,
        mint_y,
        config,
        mint_lp,
        vault_x,
        vault_y,
        treasury,
        treasury_x,
        treasury_y,
    ) = setup();
    let init_ix = create_initialise_ix(
        &mut svm, &payer, mint_x, mint_y, config, mint_lp, vault_x, vault_y, treasury, treasury_x,
        treasury_y,
    );

    let deposit_ix = create_deposit_ix(
        &mut svm, &payer, mint_x, mint_y, mint_lp, config, vault_x, vault_y,
    );

    let swap_ix = create_swap_ix(
        &mut svm, &payer, mint_x, mint_y, mint_lp, config, vault_x, vault_y, treasury, treasury_x,
        treasury_y,
    );

    let res = send(&mut svm, &[init_ix, deposit_ix, swap_ix], &payer, &[&payer]);
    assert!(res.is_ok());

    let user_x = associated_token::get_associated_token_address(&payer.pubkey(), &mint_x);
    let user_y = associated_token::get_associated_token_address(&payer.pubkey(), &mint_y);
    assert_eq!(token_balance(&svm, &vault_x), 209_970_000);
    assert_eq!(token_balance(&svm, &treasury_x), 30_000);
    assert_eq!(token_balance(&svm, &treasury_y), 0);
    assert_eq!(token_balance(&svm, &user_x), 790_000_000);
    assert!(token_balance(&svm, &vault_y) < 200_000_000);
    assert!(token_balance(&svm, &user_y) > 800_000_000);
}
