#![no_std]
use core::char::MAX;

use ft_main_io::{FTokenAction, FTokenEvent, LogicAction};
use gstd::exec::block_timestamp;
use gstd::{exec, msg, prelude::*, ActorId};
pub mod messages;
use messages::*;
use store_io::{AttributeId, AttributeStore, StoreAction, StoreEvent, TransactionId};
use tam_io::*;

static mut TAMAGOCHI: Option<Tamagotchi> = None;

const HUNGER_PER_BLOCK: u64 = 1;
const ENERGY_PER_BLOCK: u64 = 2;
const BOREDOM_PER_BLOCK: u64 = 2;

pub const FILL_PER_FEED: u64 = 2_000;
pub const FILL_PER_ENTERTAINMENT: u64 = 2_000;
pub const FILL_PER_SLEEP: u64 = 2_000;

pub const MAX_VALUE: u64 = 10_000;

#[derive(Encode, Decode, TypeInfo, Default)]
pub struct Tamagotchi {
    pub owner: ActorId,
    pub name: String,
    pub date_of_birth: u64,
    pub fed: u64,
    pub happy: u64,
    pub rested: u64,
    pub last_sleep: u64,
    pub last_feed: u64,
    pub last_play: u64,
    pub approved_account: Option<ActorId>,
    pub ft_contract_id: Option<ActorId>,
    pub ft_transaction_id: TransactionId,
    pub approve_transaction: Option<(TransactionId, ActorId, u128)>,
}

impl Tamagotchi {
    fn update_states(&mut self) {
        let current_block = block_timestamp();

        self.fed -= HUNGER_PER_BLOCK * ((current_block - self.last_feed) / 1_000);

        self.rested -= ENERGY_PER_BLOCK * ((current_block - self.last_sleep) / 1_000);

        self.happy -= BOREDOM_PER_BLOCK * ((current_block - self.last_play) / 1_000);
    }

    fn set_ftoken_contract(&mut self, ft_contract_id: ActorId) {
        self.ft_contract_id = Some(ft_contract_id);
    }

    async fn buy_attribute(&mut self, store_id: ActorId, attribute_id: AttributeId) {
        let attributes = get_attributes(&store_id).await;
        if attributes.contains(&attribute_id) {
            panic!("You have already bought that attribute");
        }
        let result = buy_attribute(&store_id, attribute_id).await;
        match result {
            Ok(_) => msg::reply(TmgEvent::AttributeBought(attribute_id), 0)
                .expect("Error in a reply `TmgEvent::AttributeBought`"),
            Err(StoreEvent::CompletePrevTx { attribute_id }) => {
                msg::reply(TmgEvent::CompletePrevPurchase(attribute_id), 0)
                    .expect("Error in a reply `TmgEvent::CompletePrevPurchase`")
            }
            _ => msg::reply(TmgEvent::ErrorDuringPurchase, 0)
                .expect("Error in a reply `TmgEvent::ErrorDuringPurchase`"),
        };
    }

    async fn approve_tokens(&mut self, account: &ActorId, amount: u128) {
        let (transaction_id, account, amount) = if let Some((
            ft_transaction_id,
            prev_account,
            prev_amount,
        )) = self.approve_transaction
        {
            if prev_account != *account || prev_amount != amount {
                panic!("Please complete the previous tx");
            } else {
                (ft_transaction_id, prev_account, prev_amount)
            }
        } else {
            let ft_transaction_id = self.ft_transaction_id;
            self.ft_transaction_id = self.ft_transaction_id.wrapping_add(1);
            self.approve_transaction = Some((ft_transaction_id, *account, amount));
            (ft_transaction_id, *account, amount)
        };

        let reply = msg::send_for_reply_as::<_, FTokenEvent>(
            self.ft_contract_id.unwrap(),
            FTokenAction::Message {
                transaction_id,
                payload: LogicAction::Approve {
                    approved_account: account,
                    amount,
                },
            },
            0,
        )
        .expect("Error in sending a message `FTokenAction::Message`")
        .await;

        self.approve_transaction = None;

        match reply {
            Ok(_) => msg::reply(TmgEvent::Approve(account), 0)
                .expect("Error in a reply `TmgEvent::Approve`"),
            Err(_) => msg::reply(TmgEvent::ApprovalError, 0)
                .expect("Error in a reply `TmgEvent::ApprovalError`"),
        };
    }
}

#[gstd::async_main]
async fn main() {
    let action: TmgAction = msg::load().expect("Error in handling msg");
    let character: &mut Tamagotchi = unsafe { TAMAGOCHI.get_or_insert(Default::default()) };

    character.update_states();

    match action {
        TmgAction::Age => {
            msg::reply(
                TmgEvent::Age(block_timestamp() - character.date_of_birth),
                0,
            )
            .expect("Error in sending Hello message to account");
        }
        TmgAction::Name => {
            msg::reply(TmgEvent::Name(character.name.to_string()), 0)
                .expect("Error in sending Hello message to account");
        }
        TmgAction::Sleep => {
            if character.rested + FILL_PER_SLEEP > MAX_VALUE {
                character.rested = MAX_VALUE;
                character.last_sleep = block_timestamp();
            } else {
                character.rested = character.rested + FILL_PER_SLEEP;
                character.last_sleep = block_timestamp();
            }
        }
        TmgAction::Feed => {
            if character.fed + FILL_PER_FEED > MAX_VALUE {
                character.fed = MAX_VALUE;
                character.last_feed = block_timestamp();
            } else {
                character.fed = character.fed + FILL_PER_FEED;
                character.last_feed = block_timestamp();
            }
        }

        TmgAction::Play => {
            if character.happy + FILL_PER_ENTERTAINMENT > MAX_VALUE {
                character.happy = MAX_VALUE;
                character.last_play = block_timestamp();
            } else {
                character.happy = character.happy + FILL_PER_ENTERTAINMENT;
                character.last_play = block_timestamp();
            }
        }
        TmgAction::Transfer(new_owner) => {
            if character.owner == msg::source() || character.approved_account == Some(msg::source())
            {
                character.owner = new_owner;
                character.approved_account = None;
            } else {
                panic!("Only the owner or an approved account can transfer ownership");
            }
        }
        TmgAction::Approve(allowed_account) => {
            if character.owner == msg::source() {
                character.approved_account = Some(allowed_account);
            } else {
                panic!("Only the owner can approve an account");
            }
        }
        TmgAction::RevokeApproval => {
            if character.owner == msg::source() {
                character.approved_account = None;
            } else {
                panic!("Only the owner can revoke approval");
            }
        }
        TmgAction::SetFTokenContract(ft_contract_id) => {
            character.set_ftoken_contract(ft_contract_id)
        }
        TmgAction::ApproveTokens { account, amount } => {
            character.approve_tokens(&account, amount).await
        }
        TmgAction::BuyAttribute {
            store_id,
            attribute_id,
        } => character.buy_attribute(store_id, attribute_id).await,
    }
}

#[no_mangle]
unsafe extern "C" fn init() {
    let name: String = msg::load().expect("Can't load init message");
    let current_block = exec::block_timestamp();

    let character = Tamagotchi {
        owner: msg::source(),
        name,
        date_of_birth: current_block,
        fed: MAX_VALUE,
        happy: MAX_VALUE,
        rested: MAX_VALUE,
        last_sleep: current_block,
        last_feed: current_block,
        last_play: current_block,
        approved_account: None,
        ..Default::default()
    };
    TAMAGOCHI = Some(character);
}

#[no_mangle]
extern "C" fn state() {
    let tamagotchi = unsafe { TAMAGOCHI.as_ref().expect("The contract is not initialized") };
    msg::reply(tamagotchi, 0).expect("Failed to share state");
}

#[no_mangle]
// It returns the Hash of metadata.
// .metahash is generating automatically while you are using build.rs
extern "C" fn metahash() {
    let metahash: [u8; 32] = include!("../.metahash");
    msg::reply(metahash, 0).expect("Failed to share metahash");
}
