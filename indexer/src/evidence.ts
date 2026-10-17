import {
  Contract,
  Keypair,
  Networks,
  TransactionBuilder,
  nativeToScVal,
  rpc,
  xdr,
} from "stellar-sdk";

export interface UploadedEvidence {
  filename: string;
  contentType: string;
  bytes: Buffer;
}

export interface PinnedEvidence {
  cid: string;
  gatewayUrl: string;
}

export function parseEvidenceMultipart(
  body: Buffer,
  contentType: string,
): UploadedEvidence {
  const boundaryMatch = /boundary=(?:"([^"]+)"|([^;]+))/i.exec(contentType);
  const boundary = boundaryMatch?.[1] || boundaryMatch?.[2]?.trim();
  if (!boundary || boundary.length > 200) throw new Error("Invalid multipart boundary");

  const marker = Buffer.from(`--${boundary}`);
  const headerSeparator = Buffer.from("\r\n\r\n");
  let cursor = 0;
  while ((cursor = body.indexOf(marker, cursor)) !== -1) {
    cursor += marker.length;
    if (body.subarray(cursor, cursor + 2).equals(Buffer.from("--"))) break;
    if (body.subarray(cursor, cursor + 2).equals(Buffer.from("\r\n"))) cursor += 2;

    const headersEnd = body.indexOf(headerSeparator, cursor);
    if (headersEnd === -1) break;
    const headers = body.subarray(cursor, headersEnd).toString("utf8");
    const disposition = /content-disposition:\s*form-data;[^\r\n]*name="file"[^\r\n]*filename="([^"]+)"/i.exec(headers);
    const nextBoundary = body.indexOf(Buffer.from(`\r\n--${boundary}`), headersEnd + 4);
    if (nextBoundary === -1) break;

    if (disposition) {
      const contentTypeMatch = /content-type:\s*([^\r\n]+)/i.exec(headers);
      const bytes = body.subarray(headersEnd + 4, nextBoundary);
      const filename = disposition[1].replace(/[\\/]/g, "_").slice(0, 255);
      const fileType = (contentTypeMatch?.[1] || "application/octet-stream").trim().toLowerCase();
      const allowedTypes = new Set(["application/pdf", "image/png", "image/jpeg", "video/mp4"]);
      if (!allowedTypes.has(fileType)) throw new Error("Unsupported evidence file type");
      const maxBytes = fileType === "video/mp4" ? 50 * 1024 * 1024 : 10 * 1024 * 1024;
      if (bytes.length === 0 || bytes.length > maxBytes) throw new Error("Evidence file exceeds size limit");
      return { filename, contentType: fileType, bytes };
    }
    cursor = nextBoundary + 2;
  }
  throw new Error("Multipart request must include a file field");
}

export async function pinEvidence(file: UploadedEvidence): Promise<PinnedEvidence> {
  const provider = process.env.IPFS_PROVIDER?.toLowerCase();
  const apiKey = process.env.IPFS_API_KEY;
  if (!apiKey || !provider) throw new Error("IPFS_PROVIDER and IPFS_API_KEY must be configured");

  const form = new FormData();
  form.append("file", new Blob([file.bytes], { type: file.contentType }), file.filename);
  let response: globalThis.Response;
  if (provider === "pinata") {
    response = await fetch(
      process.env.IPFS_UPLOAD_URL || "https://api.pinata.cloud/pinning/pinFileToIPFS",
      { method: "POST", headers: { Authorization: `Bearer ${apiKey}` }, body: form },
    );
  } else if (provider === "web3.storage") {
    response = await fetch(
      process.env.IPFS_UPLOAD_URL || "https://api.web3.storage/upload",
      { method: "POST", headers: { Authorization: `Bearer ${apiKey}` }, body: file.bytes },
    );
  } else {
    throw new Error("Unsupported IPFS_PROVIDER");
  }
  if (!response.ok) throw new Error(`IPFS provider returned HTTP ${response.status}`);
  const payload = await response.json() as { IpfsHash?: string; cid?: string };
  const cid = payload.IpfsHash || payload.cid;
  if (!cid) throw new Error("IPFS provider response did not include a CID");
  const gateway = (process.env.IPFS_GATEWAY_URL || "https://ipfs.io/ipfs/").replace(/\/$/, "");
  return { cid, gatewayUrl: `${gateway}/${cid}` };
}

export async function anchorEvidenceHash(
  disputeId: string,
  submitter: string,
  sha256: Buffer,
): Promise<string> {
  const secret = process.env.EVIDENCE_ANCHOR_SECRET;
  const contractId = process.env.PAYMENT_PROCESSOR_CONTRACT_ID;
  if (!secret || !contractId) {
    throw new Error("Evidence anchoring is not configured");
  }

  const keypair = Keypair.fromSecret(secret);
  const rpcUrl = process.env.SOROBAN_RPC_URL || "https://soroban-testnet.stellar.org";
  const networkPassphrase = process.env.STELLAR_NETWORK === "mainnet" ? Networks.PUBLIC : Networks.TESTNET;
  const server = new rpc.Server(rpcUrl, { allowHttp: rpcUrl.startsWith("http://") });
  const account = await server.getAccount(keypair.publicKey());
  const contract = new Contract(contractId);
  const transaction = new TransactionBuilder(account, {
    fee: "100000",
    networkPassphrase,
  })
    .addOperation(contract.call(
      "submit_evidence",
      nativeToScVal(keypair.publicKey(), { type: "address" }),
      nativeToScVal(disputeId, { type: "string" }),
      nativeToScVal(submitter, { type: "address" }),
      xdr.ScVal.scvBytes(sha256),
    ))
    .setTimeout(60)
    .build();
  const simulation = await server.simulateTransaction(transaction);
  if (rpc.Api.isSimulationError(simulation)) {
    throw new Error(`Evidence anchor simulation failed: ${simulation.error}`);
  }
  const prepared = rpc.assembleTransaction(transaction, simulation).build();
  prepared.sign(keypair);
  const result = await server.sendTransaction(prepared);
  if (result.status === "ERROR") throw new Error("Evidence anchor transaction was rejected");
  return result.hash;
}
