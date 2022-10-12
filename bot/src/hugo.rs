use ethers::prelude::*;
use eyre::Result;
use std::time::{SystemTime, UNIX_EPOCH};

use subway_rs::{abi, banner, uniswap, utils};

#[tokio::main]
async fn main() -> Result<()> {
    // Clear the screen and print the banner
    print!("{}[2J", 27 as char);
    println!("{}", banner::HUGO);

    // Create the websocket clieant
    let client = utils::create_websocket_client().await?;

    // Get the latest block
    let last_block = client
        .get_block(BlockNumber::Latest)
        .await?
        .unwrap()
        .number
        .unwrap();
    tracing::info!("[START] Hugo initializing on block {}...", last_block);

    // Preload our parsed addresses
    let _usdc_addr = utils::get_usdc_address()?;
    let uni_v2_addr = utils::get_univ2_address()?;
    let _sandwich_contract_address = utils::get_sandwich_contract_address()?;
    let _weth_addr = utils::get_weth_address()?;

    // Create pending stream
    let stream = if let Ok(c) = client.watch_pending_transactions().await {
        c
    } else {
        panic!("Failed to create filter watcher for pending transactions!");
    };

    // Create transaction stream
    let mut tx_stream = stream.transactions_unordered(usize::MAX);

    // Listening to pending transactions
    while let Some(tx) = tx_stream.next().await {
        // println!("[IN] Received pending transaction: {:?}", tx);

        // Unwrap the transaction
        let tx = match tx {
            Ok(tx) => tx,
            Err(e) => {
                tracing::debug!("Transaction error: {:?}", e);
                continue;
            }
        };

        // Get the transaction receipt
        match client.get_transaction_receipt(tx.hash).await {
            Ok(Some(r)) => {
                tracing::debug!("Found transaction receipt {:?}, skipping...", r);
                continue;
            }
            Err(e) => {
                tracing::error!("{:?}", e);
                continue;
            }
            Ok(None) => { /* No Transaction, we can proceed with sandwiching */ }
        }

        // Match on tx.to
        if tx.to != Some(uni_v2_addr) {
            tracing::debug!("Transaction is not to uniswap v2, skipping...");
            continue;
        }

        // Decode the transaction data
        let decoded = if let Ok(d) = abi::decode_uniswap_router_calldata(&tx.input) {
            d
        } else {
            tracing::debug!("Failed to decode transaction data, skipping...");
            continue;
        };

        // We don't want to sandwich uniswap calls with expired deadlines
        let deadline = decoded.deadline;
        let start = SystemTime::now();
        let since_the_epoch = start
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();
        println!(
            "Comparing deadline: {} to current time: {}",
            deadline, since_the_epoch
        );
        if U256::from(since_the_epoch) > deadline {
            tracing::debug!("Transaction deadline has expired, skipping...");
            continue;
        }

        tracing::info!(
            "[DETECTED] Potential sandwichable transaction: {:#?}",
            decoded
        );

        // Calculate sandwichability
        // NOTE: Token A will always be WETH here since the call is decoded as a SwapExactETHForTokensCall
        let token_a = decoded.path[0];
        let token_b = decoded.path[1];
        let _pair_to_sandwich =
            if let Ok(p) = uniswap::get_uniswap_v2_pair_address(&token_a, &token_b) {
                p
            } else {
                tracing::debug!(
                    "Failed to get uniswap v2 pair address for tokens [{:?}, {:?}], skipping...",
                    token_a,
                    token_b
                );
                continue;
            };

        // const [reserveWeth, reserveToken] = await getUniv2Reserve(
        //     pairToSandwich,
        //     weth,
        //     token
        // );
        // const optimalWethIn = calcSandwichOptimalIn(
        //     userAmountIn,
        //     userMinRecv,
        //     reserveWeth,
        //     reserveToken
        // );
    }

    Ok(())
}
