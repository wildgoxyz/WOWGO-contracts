use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, Burn, Transfer};
use solana_program::{
    account_info::AccountInfo, 
    program_pack::Pack
};
use spl_token::state::{Mint, Account as TokenAccount, AccountState};
use dotenv::dotenv;
use std::env;

declare_id!("PROGRAM_ID");

const MAX_TOTAL_SUPPLY: u64 = 77_000_000_000;
const INITIAL_MINTABLE_SUPPLY: u64 = 10_000_000_000;

#[program]
pub mod wowgo {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        ctx.accounts.state.can_mint_more = true;
        ctx.accounts.state.transfer_fee_percent = 2;
        ctx.accounts.state.admin = *ctx.accounts.payer.key; 
        msg!("Program initialized successfully!");
        Ok(())
    }
    

    pub fn transfer_tokens(ctx: Context<TransferTokens>, amount: u64) -> Result<()> {
        require!(amount > 0, CustomError::InvalidAmount);
    
        let fee_percent = ctx.accounts.state.transfer_fee_percent as u64;
        let fee_amount = amount
            .checked_mul(fee_percent)
            .ok_or(CustomError::Overflow)?
            .checked_div(100)
            .ok_or(CustomError::Overflow)?;
        let transfer_amount = amount
            .checked_sub(fee_amount)
            .ok_or(CustomError::Overflow)?;
    
        require!(transfer_amount > 0, CustomError::InvalidAmount);
    
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.from.to_account_info(),
                    to: ctx.accounts.to.to_account_info(),
                    authority: ctx.accounts.authority.to_account_info(),
                },
            ),
            transfer_amount,
        )?;
    
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.from.to_account_info(),
                    to: ctx.accounts.fee_receiver.to_account_info(),
                    authority: ctx.accounts.authority.to_account_info(),
                },
            ),
            fee_amount,
        )?;
    
        emit!(TokensTransferred {
            sender: ctx.accounts.from.key(),
            recipient: ctx.accounts.to.key(),
            amount: transfer_amount,
        });
    
        emit!(TransferFeeCharged {
            sender: ctx.accounts.from.key(),
            fee_receiver: ctx.accounts.fee_receiver.key(),
            fee_amount,
        });
    
        Ok(())
    }
    
    pub fn mint_tokens(ctx: Context<MintTokens>, amount: u64) -> Result<()> {
        require!(amount > 0, CustomError::InvalidAmount);
        require!(
            ctx.accounts.admin.key() == ctx.accounts.state.admin,
            CustomError::Unauthorized
        );

        let mint_info = &ctx.accounts.mint;
        let to_info = &ctx.accounts.to;

        let mint_data = Mint::unpack(&mint_info.try_borrow_data()?)
            .map_err(|_| CustomError::InvalidMintAccount)?;

        require!(
            mint_data.mint_authority.is_some(),
            CustomError::MintAuthorityMissing
        );

        let to_data = TokenAccount::unpack(&to_info.try_borrow_data()?)
            .map_err(|_| CustomError::InvalidTokenAccount)?;

        require!(
            to_data.state == AccountState::Initialized,
            CustomError::AccountNotInitialized
        );
        require!(
            to_data.mint == mint_info.key(),
            CustomError::InvalidRecipientMint
        );
        require!(
            to_info.owner == &spl_token::id(),
            CustomError::InvalidTokenAccount
        );


        require!(
            mint_data.mint_authority.ok_or(CustomError::MintAuthorityMissing)? == ctx.accounts.mint_auth.key(),
            CustomError::Unauthorized
        );
        

        let max_mintable = if ctx.accounts.state.can_mint_more {
            MAX_TOTAL_SUPPLY
        } else {
            INITIAL_MINTABLE_SUPPLY
        };

        require!(
            mint_data.supply.checked_add(amount).ok_or(CustomError::Overflow)? <= max_mintable,
            CustomError::ExceedsTotalSupply
        );

        let mint_key = mint_info.key(); 
        let seeds: &[&[u8]] = &[b"mint_auth", mint_key.as_ref(), &[ctx.bumps.mint_auth]];

        token::mint_to(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                token::MintTo {
                    mint: mint_info.to_account_info(),
                    to: to_info.to_account_info(),
                    authority: ctx.accounts.mint_auth.to_account_info(),
                },
                &[seeds],
            ),
            amount,
        )?;

        emit!(TokensMinted {
            recipient: to_info.key(),
            amount,
        });

        Ok(())
    }


    pub fn burn_tokens(ctx: Context<BurnTokens>, amount: u64) -> Result<()> {
        require!(amount > 0, CustomError::InvalidAmount);

        let mint_info = &ctx.accounts.mint;
        let from_info = &ctx.accounts.from;


        let from_data = TokenAccount::unpack(&from_info.try_borrow_data()?)
            .map_err(|_| CustomError::InvalidTokenAccount)?;

        require!(
            from_data.mint == mint_info.key(),
            CustomError::InvalidRecipientMint
        );

        token::burn(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Burn {
                    mint: mint_info.to_account_info(),
                    from: from_info.to_account_info(),
                    authority: ctx.accounts.authority.to_account_info(),
                },
            ),
            amount,
        )?;

        emit!(TokensBurned {
            owner: from_info.key(),
            amount,
        });

        Ok(())
    }   

    pub fn set_transfer_fee(ctx: Context<SetTransferFee>, new_fee_percent: u8) -> Result<()> {
        require!(new_fee_percent <= 10, CustomError::FeeTooHigh);
        ctx.accounts.state.transfer_fee_percent = new_fee_percent;
        Ok(())
    }

    pub fn set_minting_status(ctx: Context<SetMintingStatus>, can_mint: bool) -> Result<()> {
        ctx.accounts.state.can_mint_more = can_mint;
        Ok(())
    }
}

#[derive(Accounts)]
pub struct SetTransferFee<'info> {
    #[account(mut, has_one = admin)]
    pub state: Account<'info, TokenState>,

    #[account(signer)]
    /// CHECK: `admin` is owner 
    pub admin: AccountInfo<'info>, 
}

#[event]
pub struct TransferFeeCharged {
    pub sender: Pubkey,
    pub fee_receiver: Pubkey,
    pub fee_amount: u64,
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(init, payer = payer, space = 8 + 1 + 1 + 32, seeds = [b"state"], bump)]
    pub state: Account<'info, TokenState>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct MintTokens<'info> {
    #[account(mut, owner = token::ID)]
    /// CHECK: This is a token mint account, manually validated with `unpack()`
    pub mint: AccountInfo<'info>,  

    #[account(mut, owner = token::ID)]
    /// CHECK: This is a recipient account for the minted tokens, manually validated with `unpack()`
    pub to: AccountInfo<'info>,  

    #[account(
        seeds = [b"mint_auth", mint.key().as_ref()], 
        bump
    )]
    /// CHECK: This is a PDA acting as the mint authority
    pub mint_auth: AccountInfo<'info>,  

    #[account(mut, seeds = [b"state"], bump, has_one = admin)]
    pub state: Account<'info, TokenState>,

    #[account(signer)]
    /// CHECK: Only admin can mint tokens
    pub admin: AccountInfo<'info>,

    #[account(address = token::ID)]
    pub token_program: Program<'info, Token>,
}


#[derive(Accounts)]
pub struct BurnTokens<'info> {
    #[account(mut, owner = token::ID)]
    /// CHECK: This is a token mint account, manually validated with `unpack()`
    pub mint: AccountInfo<'info>,  

    #[account(mut, owner = token::ID)]
    /// CHECK: This is a token account of the owner, manually validated with `unpack()`
    pub from: AccountInfo<'info>,  

    #[account(signer)]
    /// CHECK: The authority of the token account
    pub authority: AccountInfo<'info>,  

    #[account(address = token::ID)]
    pub token_program: Program<'info, Token>,
}


#[derive(Accounts)]
pub struct TransferTokens<'info> {
    #[account(mut, owner = token::ID)]
    /// CHECK: The source account for transferring tokens, validated by program logic
    pub from: AccountInfo<'info>,  

    #[account(mut, owner = token::ID)]
    /// CHECK: The destination account receiving tokens, validated by program logic
    pub to: AccountInfo<'info>,  

    #[account(mut, owner = token::ID)]
    /// CHECK: The account receiving the transfer fee
    pub fee_receiver: AccountInfo<'info>,  

    #[account(signer)]
    /// CHECK: Authority of the source account
    pub authority: AccountInfo<'info>,  

    #[account(mut, seeds = [b"state"], bump)]
    pub state: Account<'info, TokenState>,

    #[account(address = token::ID)]
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct SetMintingStatus<'info> {
    #[account(mut, has_one = admin)]
    pub state: Account<'info, TokenState>,

    #[account(signer)]
    /// CHECK: `admin` is owner 
    pub admin: AccountInfo<'info>,
}

#[account]
pub struct TokenState {
    pub can_mint_more: bool,
    pub transfer_fee_percent: u8,
    
    /// CHECK: `admin` is a `Pubkey` set during program initialization and cannot be changed
    pub admin: Pubkey, 
}

#[event]
pub struct TokensMinted {
    pub recipient: Pubkey,
    pub amount: u64,
}

#[event]
pub struct TokensBurned {
    pub owner: Pubkey,
    pub amount: u64,
}

#[event]
pub struct TokensTransferred {
    pub sender: Pubkey,
    pub recipient: Pubkey,
    pub amount: u64,
}


#[error_code]
pub enum CustomError {
    #[msg("Invalid amount, must be greater than zero.")]
    InvalidAmount,
    #[msg("Unauthorized action.")]
    Unauthorized,
    #[msg("Overflow error.")]
    Overflow,
    #[msg("Exceeds total supply limit.")]
    ExceedsTotalSupply,
    #[msg("Invalid mint account.")]
    InvalidMintAccount,
    #[msg("Invalid token account.")]
    InvalidTokenAccount,
    #[msg("Recipient token account does not match mint.")]
    InvalidRecipientMint,
    #[msg("Mint authority is missing.")]
    MintAuthorityMissing,
    #[msg("Token account is not initialized.")]
    AccountNotInitialized,
    #[msg("Transfer fee is too high, must be <= 10%.")]
    FeeTooHigh,

}
