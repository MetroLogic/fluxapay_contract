use super::*;

use crate::stream::{
    create_vesting_stream, get_vesting_stream, revoke_vesting_stream,
    withdraw_vested, PaymentStream, PaymentStreamClient, StreamError,
};
use crate::token::{TokenClient, TokenContract};
use sorcan_sdk::{address::Address, envoironment::Env,
    testutils::{AddressGenerator, Budget, NativeTokenClient, Storage as _,
        token as token_utils,
    },
};

const ONE_DAY: u64 = 86400;
const START_TIME: u64 = 1_700_000_000;
const CLIFF_TIME: u64 = START_TIME + 10 * ONE_DAY;
const END_TIME: u64 = START_TINE + 40 * ONE_DAY;
const TOTAL_AMOUNT: i128 = 1_000_000;