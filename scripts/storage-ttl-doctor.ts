import {
  Contract,
  SorobanRpc,
  xdr,
  scValToNative,
  Account,
  TransactionBuilder,
  Networks,
  Address
} from "@stellar/stellar-sdk";

export interface CliOptions {
  threshold: number | null;
  rpcUrl: string;
  vaultId: string | null;
  settlementId: string | null;
  revenuePoolId: string | null;
  requestIds: string[];
  developerAddresses: string[];
}

/**
 * One entry returned by a contract's TTL view.
 *
 * `ttl` is optional: the revenue pool's `get_ttl_policy` reports policy
 * constants only and carries no live TTL, because contract code cannot observe
 * the remaining TTL of a ledger entry. Revenue-pool entries get their live TTL
 * attached from Soroban RPC `getLedgerEntries` (see `fetchLiveInstanceTtl`).
 */
export interface StorageEntryTtl {
  category: string;
  key_desc: string;
  storage_type: string;
  ttl?: number;
  threshold: number;
  bump_amount: number;
}

export interface ReportEntry {
  contract: string;
  contract_id: string;
  key_desc: string;
  ttl: number;
  threshold: number;
  bump_amount: number;
}

export interface CategoryReport {
  storage_type: string;
  remaining_ttl: number | null;
  threshold: number | null;
  bump_amount: number | null;
  status: "OK" | "WARN" | "EMPTY" | "ERROR";
  entries: ReportEntry[];
}

export interface DoctorReport {
  timestamp: string;
  threshold: number | null;
  summary: {
    total_categories: number;
    categories_below_threshold: number;
    status: "OK" | "WARN" | "ERROR";
  };
  categories: Record<string, CategoryReport>;
  errors: string[];
}

// Parse CLI arguments manually to avoid dependency complexity and ensure testability
export function parseArgs(args: string[]): CliOptions {
  const options: CliOptions = {
    threshold: null,
    rpcUrl: "https://soroban-testnet.stellar.org",
    vaultId: null,
    settlementId: null,
    revenuePoolId: null,
    requestIds: [],
    developerAddresses: [],
  };

  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (arg === "--threshold") {
      const val = parseInt(args[++i], 10);
      options.threshold = isNaN(val) ? null : val;
    } else if (arg === "--rpc-url") {
      options.rpcUrl = args[++i];
    } else if (arg === "--vault-id") {
      options.vaultId = args[++i];
    } else if (arg === "--settlement-id") {
      options.settlementId = args[++i];
    } else if (arg === "--revenue-pool-id") {
      options.revenuePoolId = args[++i];
    } else if (arg === "--request-ids") {
      const val = args[++i];
      options.requestIds = val ? val.split(",").map(s => s.trim()).filter(Boolean) : [];
    } else if (arg === "--developer-addresses") {
      const val = args[++i];
      options.developerAddresses = val ? val.split(",").map(s => s.trim()).filter(Boolean) : [];
    }
  }
  return options;
}

// Convert string list to Symbol vector ScVal
export function stringsToSymbolVec(strings: string[]): xdr.ScVal {
  return xdr.ScVal.scvVec(strings.map(s => xdr.ScVal.scvSymbol(s)));
}

// Convert address list to Address vector ScVal
export function addressesToAddressVec(addresses: string[]): xdr.ScVal {
  return xdr.ScVal.scvVec(addresses.map(addr => Address.fromString(addr).toScVal()));
}

// Helper to build a transaction for simulation
export function buildSimulationTx(
  contractId: string,
  method: string,
  args: xdr.ScVal[],
  networkPassphrase: string
) {
  const dummyAccount = new Account("GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF", "0");
  const contract = new Contract(contractId);
  return new TransactionBuilder(dummyAccount, {
    fee: "100",
    networkPassphrase,
  })
    .addOperation(contract.call(method, ...args))
    .setTimeout(30)
    .build();
}

/**
 * Read the live TTL of a deployed contract's instance entry over Soroban RPC.
 *
 * Contract code cannot observe the remaining TTL of a ledger entry, so live
 * TTLs must be read from the ledger itself. `getLedgerEntries` returns each
 * entry's `liveUntilLedgerSeq`, and the response carries `latestLedger`, so the
 * remaining TTL is `liveUntilLedgerSeq - latestLedger`.
 *
 * @param server  a connected Soroban RPC server
 * @param contractId  the deployed contract whose instance entry to inspect
 * @param errors  collects a human-readable reason when the TTL cannot be read
 * @returns `{ ttl }` in ledgers, or `null` when the entry (or its
 *   `liveUntilLedgerSeq`) is unavailable
 */
export async function fetchLiveInstanceTtl(
  server: SorobanRpc.Server,
  contractId: string,
  errors: string[] = []
): Promise<{ ttl: number } | null> {
  try {
    const instanceKey = new Contract(contractId).getFootprint();
    const response = await server.getLedgerEntries(instanceKey);

    if (!response.entries || response.entries.length === 0) {
      errors.push(`No ledger entry returned for contract instance (${contractId})`);
      return null;
    }

    const entry = response.entries[0];
    if (typeof entry.liveUntilLedgerSeq !== "number") {
      errors.push(
        `Ledger entry for ${contractId} has no liveUntilLedgerSeq; cannot compute a remaining TTL`
      );
      return null;
    }

    return { ttl: entry.liveUntilLedgerSeq - response.latestLedger };
  } catch (err: any) {
    errors.push(`Failed to read live TTL for ${contractId}: ${err.message || err}`);
    return null;
  }
}

async function queryContractTtl(
  server: SorobanRpc.Server,
  networkPassphrase: string,
  contractId: string,
  contractName: string,
  method: string,
  args: xdr.ScVal[],
  errors: string[]
): Promise<ReportEntry[]> {
  try {
    const tx = buildSimulationTx(contractId, method, args, networkPassphrase);
    const sim = await server.simulateTransaction(tx);

    if (SorobanRpc.Api.isSimulationError(sim)) {
      errors.push(`Simulation failed for ${contractName} (${contractId}): ${sim.error}`);
      return [];
    }

    const retval = sim.result?.retval;
    if (!retval) {
      errors.push(`No return value in simulation for ${contractName} (${contractId})`);
      return [];
    }

    const nativeResult = scValToNative(retval);
    if (!Array.isArray(nativeResult)) {
      errors.push(`Malformed return value in simulation for ${contractName} (${contractId})`);
      return [];
    }

    return nativeResult.map((entry: any) => {
      const mapped: Record<string, unknown> = {
        contract: contractName,
        contract_id: contractId,
        key_desc: String(entry.key_desc),
        threshold: Number(entry.threshold),
        bump_amount: Number(entry.bump_amount),
        // Map category name to string cleanly
        category: String(entry.category),
      };
      // Policy-only views (e.g. the revenue pool's `get_ttl_policy`) carry no
      // in-band TTL; the live TTL is attached from `getLedgerEntries` instead.
      if (entry.ttl !== undefined && entry.ttl !== null) {
        mapped.ttl = Number(entry.ttl);
      }
      return mapped;
    }) as unknown as (ReportEntry & { category: string })[];
  } catch (err: any) {
    errors.push(`Failed to query ${contractName} (${contractId}): ${err.message || err}`);
    return [];
  }
}

export async function run() {
  const options = parseArgs(process.argv.slice(2));
  const errors: string[] = [];

  const networkPassphrase = Networks.TESTNET; // Default to testnet passphrase
  const server = new SorobanRpc.Server(options.rpcUrl);

  const rawEntries: (ReportEntry & { category: string })[] = [];

  // Query Vault
  if (options.vaultId) {
    const vaultArgs = [stringsToSymbolVec(options.requestIds)];
    const entries = await queryContractTtl(
      server,
      networkPassphrase,
      options.vaultId,
      "Vault",
      "get_storage_ttl",
      vaultArgs,
      errors
    );
    rawEntries.push(...(entries as any));
  }

  // Query Settlement
  if (options.settlementId) {
    const settlementArgs = [addressesToAddressVec(options.developerAddresses)];
    const entries = await queryContractTtl(
      server,
      networkPassphrase,
      options.settlementId,
      "Settlement",
      "get_storage_ttl",
      settlementArgs,
      errors
    );
    rawEntries.push(...(entries as any));
  }

  // Query Revenue Pool
  //
  // The pool exposes TTL *policy* only (`get_ttl_policy`): it reports the
  // threshold and bump constants it applies, and deliberately has no `ttl`
  // field, because a contract cannot observe the remaining TTL of a ledger
  // entry. The live instance TTL therefore comes from RPC `getLedgerEntries`.
  if (options.revenuePoolId) {
    const policy = await queryContractTtl(
      server,
      networkPassphrase,
      options.revenuePoolId,
      "RevenuePool",
      "get_ttl_policy",
      [],
      errors
    );

    if (policy.length > 0) {
      const live = await fetchLiveInstanceTtl(server, options.revenuePoolId, errors);
      if (live !== null) {
        rawEntries.push(...(policy as any[]).map((entry) => ({ ...entry, ttl: live.ttl })));
      }
    }
  }

  // Group and Aggregate
  const categories: Record<string, CategoryReport> = {};

  // Initialize known categories to handle "empty categories" or standard reports cleanly
  const knownCategories = ["Instance", "ProcessedRequest", "DeveloperBalance", "WithdrawalToday", "DailyWithdrawCap"];
  for (const cat of knownCategories) {
    // Determine storage type based on category
    const storageType = cat === "Instance" ? "Instance" : "Persistent";
    categories[cat] = {
      storage_type: storageType,
      remaining_ttl: null,
      threshold: null,
      bump_amount: null,
      status: "EMPTY",
      entries: [],
    };
  }

  // Group raw entries
  for (const entry of rawEntries) {
    if (typeof entry.ttl !== "number" || !Number.isFinite(entry.ttl)) {
      errors.push(
        `Missing live TTL for ${entry.contract} (${entry.contract_id}) category ${entry.category}`
      );
      continue;
    }

    const cat = entry.category;
    if (!categories[cat]) {
      categories[cat] = {
        storage_type: cat === "Instance" ? "Instance" : "Persistent",
        remaining_ttl: null,
        threshold: null,
        bump_amount: null,
        status: "EMPTY",
        entries: [],
      };
    }
    categories[cat].entries.push({
      contract: entry.contract,
      contract_id: entry.contract_id,
      key_desc: entry.key_desc,
      ttl: entry.ttl,
      threshold: entry.threshold,
      bump_amount: entry.bump_amount,
    });
  }

  let categoriesBelowThreshold = 0;
  let hasErrors = errors.length > 0;

  // Aggregate each category
  for (const cat in categories) {
    const report = categories[cat];
    if (report.entries.length === 0) {
      report.status = "EMPTY";
      continue;
    }

    // Minimum remaining TTL of all entries in the category
    let minTtl = Infinity;
    let categoryThreshold = 0;
    let categoryBumpAmount = 0;

    for (const entry of report.entries) {
      if (entry.ttl < minTtl) {
        minTtl = entry.ttl;
        categoryThreshold = entry.threshold;
        categoryBumpAmount = entry.bump_amount;
      }
    }

    report.remaining_ttl = minTtl;
    report.threshold = categoryThreshold;
    report.bump_amount = categoryBumpAmount;

    // Check against CLI threshold if provided, else use the entry's default threshold
    const checkThreshold = options.threshold !== null ? options.threshold : categoryThreshold;
    if (minTtl < checkThreshold) {
      report.status = "WARN";
      categoriesBelowThreshold++;
    } else {
      report.status = "OK";
    }
  }

  // Overall status
  let overallStatus: "OK" | "WARN" | "ERROR" = "OK";
  if (hasErrors) {
    overallStatus = "ERROR";
  } else if (categoriesBelowThreshold > 0) {
    overallStatus = "WARN";
  }

  const finalReport: DoctorReport = {
    timestamp: new Date().toISOString(),
    threshold: options.threshold,
    summary: {
      total_categories: Object.keys(categories).length,
      categories_below_threshold: categoriesBelowThreshold,
      status: overallStatus,
    },
    categories,
    errors,
  };

  console.log(JSON.stringify(finalReport, null, 2));

  // Return appropriate exit codes for CI if thresholds are exceeded or errors occurred
  if (hasErrors) {
    process.exit(1);
  }
  if (categoriesBelowThreshold > 0) {
    process.exit(1);
  }
  process.exit(0);
}

if (require.main === module) {
  run();
}
