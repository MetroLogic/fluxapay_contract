/**
 * Backend Contract Invoke Example
 *
 * This example demonstrates how a backend service invokes a Fluxapay contract
 * function via Soroban RPC. It covers building a transaction, signing with a
 * server keypair, submitting it to the network, and handling the response.
 *
 * Prerequisites:
 *   - Node.js and npm installed
 *   - @stellar/stellar-sdk installed
 *   - STELLAR_PRIVATE_KEY and STELLAR_RPC_URL environment variables set
 *   - PAYMENT_PROCESSOR_ID environment variable (contract address)
 *
 * Usage:
 *   npx ts-node backend-invoke-example.ts
 *
 * Environment variables:
 *   STELLAR_PRIVATE_KEY - Server keypair secret (starts with S)
 *   STELLAR_RPC_URL - Soroban RPC endpoint (e.g. https://soroban-testnet.stellar.org)
 *   STELLAR_NETWORK - Network passphrase (e.g. "Test SDF Network ; September 2015")
 *   PAYMENT_PROCESSOR_ID - Fluxapay contract address
 */

import {
  Keypair,
  SorobanDataBuilder,
  TransactionBuilder,
  BASE_FEE,
  Networks,
  nativeToScVal,
  scValToNative,
  Address,
  StrKey,
  xdr,
} from "@stellar/stellar-sdk";

// Load environment variables
const privateKey = process.env.STELLAR_PRIVATE_KEY;
if (!privateKey) {
  throw new Error("STELLAR_PRIVATE_KEY environment variable is required");
}

const rpcUrl = process.env.STELLAR_RPC_URL;
if (!rpcUrl) {
  throw new Error("STELLAR_RPC_URL environment variable is required");
}

const networkPassphrase =
  process.env.STELLAR_NETWORK || Networks.TESTNET_NETWORK_PASSPHRASE;
const contractId = process.env.PAYMENT_PROCESSOR_ID;
if (!contractId) {
  throw new Error("PAYMENT_PROCESSOR_ID environment variable is required");
}

/**
 * Step 1: Initialize keypair and server connection
 *
 * The keypair represents the server's identity—the account that signs transactions
 * on behalf of the backend service.
 */
const serverKeypair = Keypair.fromSecret(privateKey);
console.log(`[INFO] Server public key: ${serverKeypair.publicKey()}`);

/**
 * Step 2: Build a contract invocation transaction
 *
 * This example invokes the get_payment function to fetch payment status.
 * Adjust function name, args, and return type based on your contract.
 *
 * Breakdown:
 *   - accountSequence: Current sequence number of the signing account (fetched from network)
 *   - build(): Constructs the transaction envelope
 *   - toXDR(): Serializes to XDR for network submission
 */
async function invokeContract() {
  try {
    // Step 2a: Fetch account details from the network
    console.log(`[INFO] Fetching account from ${rpcUrl}...`);
    const accountResponse = await fetch(rpcUrl, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        jsonrpc: "2.0",
        id: "1",
        method: "getAccount",
        params: {
          account: serverKeypair.publicKey(),
        },
      }),
    });

    const accountData = await accountResponse.json();
    if (accountData.error) {
      throw new Error(
        `Failed to fetch account: ${accountData.error.message}`
      );
    }

    const sequence = accountData.result.sequence;
    console.log(`[INFO] Account sequence: ${sequence}`);

    // Step 2b: Build the transaction
    // TransactionBuilder orchestrates the transaction structure, including:
    //   - Fee: BASE_FEE (100 stroops) per operation + overhead
    //   - Sequence: Incremented for each transaction by this account
    //   - Network: Testnet or public network passphrase
    const transaction = new TransactionBuilder(
      {
        publicKey: serverKeypair.publicKey(),
        sequence: sequence,
      },
      {
        fee: BASE_FEE,
        networkPassphrase: networkPassphrase,
      }
    )
      // addOperation() adds a contract invocation operation
      // Contract invocation parameters:
      //   - contractId: The contract address (public key hash format)
      //   - method: Function name to invoke (e.g., "get_payment")
      //   - args: Array of arguments, each wrapped in nativeToScVal()
      //   - source: Optional—if omitted, uses the transaction source
      .addOperation(
        // Adjust this operation based on your contract function.
        // Example: invoke get_payment(payment_id: String) -> PaymentCharge
        {
          type: "invokeHostFunction",
          hostFunction: xdr.HostFunction.hostFunctionTypeInvokeContract([
            // Contract address
            Address.fromString(contractId).toScVal(),
            // Method name
            nativeToScVal("get_payment"),
            // Arguments (payment_id: String)
            nativeToScVal("inv_20260330_001"),
          ]),
          // Soroban spec requires auth array (typically empty for read-only calls)
          auth: [],
        }
      )
      .setTimeout(300) // 5 minutes
      .build();

    console.log(`[INFO] Transaction built (sequence: ${sequence})`);

    // Step 3: Sign the transaction with the server keypair
    // Signing ensures the network validates that the server authorized this operation.
    // The Keypair.sign() method uses Ed25519 signing.
    transaction.sign(serverKeypair);
    console.log(`[INFO] Transaction signed`);

    // Step 4: Submit the signed transaction to Soroban RPC
    // The network validates the signature and executes the contract function.
    console.log(`[INFO] Submitting transaction to ${rpcUrl}...`);
    const submitResponse = await fetch(rpcUrl, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        jsonrpc: "2.0",
        id: "2",
        method: "sendTransaction",
        params: {
          transaction: transaction.toEnvelope().toXDR("base64"),
        },
      }),
    });

    const submitData = await submitResponse.json();
    if (submitData.error) {
      throw new Error(`Submit failed: ${submitData.error.message}`);
    }

    // Step 5: Poll the network for transaction result
    // Soroban transactions are asynchronous; poll getTransaction to retrieve the result.
    const txHash = submitData.result.hash;
    console.log(`[INFO] Transaction submitted (hash: ${txHash})`);
    console.log(`[INFO] Polling for result...`);

    // Wait a moment before polling (network needs time to process)
    await new Promise((resolve) => setTimeout(resolve, 3000));

    // Poll for result (with exponential backoff for robustness)
    let attempts = 0;
    const maxAttempts = 20;
    let result = null;

    while (attempts < maxAttempts) {
      const resultResponse = await fetch(rpcUrl, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          jsonrpc: "2.0",
          id: "3",
          method: "getTransaction",
          params: { hash: txHash },
        }),
      });

      const resultData = await resultResponse.json();
      if (resultData.result && resultData.result.status) {
        result = resultData.result;
        break;
      }

      attempts++;
      const waitTime = Math.min(1000 * Math.pow(1.5, attempts), 5000);
      console.log(`[INFO] Attempt ${attempts}/${maxAttempts}, retrying in ${waitTime}ms...`);
      await new Promise((resolve) => setTimeout(resolve, waitTime));
    }

    if (!result) {
      throw new Error("Transaction result not found after polling");
    }

    console.log(`[INFO] Transaction status: ${result.status}`);

    // Step 6: Handle the response based on transaction status
    // Possible statuses:
    //   - SUCCESS: Transaction executed; result is in resultXdr
    //   - FAILED: Transaction failed; error details in resultXdr
    //   - PENDING: Still processing (unlikely if polling correctly)
    if (result.status === "SUCCESS") {
      console.log(`[SUCCESS] Contract invocation succeeded`);

      // Decode the result from XDR
      if (result.resultXdr) {
        const txResult = xdr.TransactionResult.fromXDR(
          result.resultXdr,
          "base64"
        );
        // Extract result data (structure depends on contract return type)
        console.log(`[INFO] Result XDR: ${result.resultXdr.substring(0, 100)}...`);
        console.log(`[INFO] Parse resultXdr with your contract's return type.`);
      }
    } else if (result.status === "FAILED") {
      console.error(`[ERROR] Contract invocation failed`);
      if (result.resultXdr) {
        console.error(`[ERROR] Result XDR: ${result.resultXdr}`);
      }
    } else {
      console.warn(`[WARN] Unexpected status: ${result.status}`);
    }

    return result;
  } catch (error) {
    console.error(`[ERROR] ${error instanceof Error ? error.message : String(error)}`);
    process.exit(1);
  }
}

// Execute the invocation
invokeContract();
