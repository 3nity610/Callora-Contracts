# Storage TTL Doctor Utility

The Storage TTL Doctor is a CLI utility that reports the remaining Time-To-Live (TTL) of the Callora smart contracts' storage entries so operators can trigger extensions (bumps) before data is archived.

In Soroban, storage entries (such as instance config or developer balances in persistent storage) are automatically archived once their TTL expires.

---

## Where live TTLs come from

**Live TTLs are read from Soroban RPC `getLedgerEntries`, not from a contract view.**

Each `LedgerEntry` returned by `getLedgerEntries` carries:

* `liveUntilLedgerSeq` — the future ledger number at which the entry expires;
* and the enclosing response carries `latestLedger`.

Remaining TTL is therefore `liveUntilLedgerSeq - latestLedger`.

This split exists because **contract code cannot observe the remaining TTL of a ledger entry**. A contract view that claims to report one can only return a constant, so an operator cannot distinguish a healthy entry from one about to be archived. The `scripts/storage-ttl-doctor.ts` revenue-pool path reads the policy from the contract and the live TTL from `getLedgerEntries`.

`docs/interfaces/revenue_pool.json` and the contract source are the source of truth for the view signatures below.

---

## View Endpoints in Smart Contracts

### Revenue Pool — policy only

`get_ttl_policy() -> Vec<TtlPolicy>`

`TtlPolicy` carries `category`, `key_desc`, `storage_type`, `threshold`, and `bump_amount`. It has **no `ttl` field**: the revenue pool previously exposed `get_storage_ttl()`, whose `ttl` field was the live instance TTL under `cfg(test)` but the constant `BUMP_AMOUNT` in production builds — a fabricated measurement that made the tool useless (and misleading) in exactly the deployments it was meant to check. For the deployed revenue pool, the doctor reads `liveUntilLedgerSeq` for the contract instance entry over RPC.

### Vault and Settlement — entries with in-band TTL

* **Vault**: `get_storage_ttl(request_ids: Vec<Symbol>) -> Vec<StorageEntryTtl>`
* **Settlement**: `get_storage_ttl(developer_addresses: Vec<Address>) -> Vec<StorageEntryTtl>`

These views exist to *enumerate* which storage entries belong to a category (the doctor cannot discover persistent keys on its own). Their `ttl` fields carry the same caveat as the removed revenue-pool field and should be migrated to the same RPC-based approach; until then, treat the reported `ttl` as advisory and cross-check with `getLedgerEntries`.

---

## How to Run Locally

### 1. Install Node.js Dependencies

Run the following command at the root of the project:

```bash
npm install
```

### 2. Run the Doctor Script

You can execute the script using `ts-node` or npm run scripts:

```bash
npx ts-node scripts/storage-ttl-doctor.ts \
  --vault-id "C..." \
  --settlement-id "C..." \
  --revenue-pool-id "C..." \
  --threshold 100000
```

---

## CLI Options

| Option | Description | Default |
|--------|-------------|---------|
| `--threshold <number>` | Min remaining TTL (in ledgers) below which the script exits with code 1 | Uses contract default thresholds |
| `--rpc-url <string>` | Soroban RPC server endpoint | `https://soroban-testnet.stellar.org` |
| `--vault-id <string>` | Contract ID of the deployed Callora Vault | `null` |
| `--settlement-id <string>` | Contract ID of the deployed Callora Settlement | `null` |
| `--revenue-pool-id <string>` | Contract ID of the deployed Callora Revenue Pool | `null` |
| `--request-ids <list>` | Comma-separated list of transaction request IDs to query processed status TTL | `[]` |
| `--developer-addresses <list>`| Comma-separated list of developer addresses to check persistent balance TTL | `[]` (falls back to index) |

---

## JSON Schema

The tool outputs a machine-readable JSON report to stdout:

```json
{
  "timestamp": "2026-06-28T00:10:00.000Z",
  "threshold": 100000,
  "summary": {
    "total_categories": 5,
    "categories_below_threshold": 0,
    "status": "OK"
  },
  "categories": {
    "Instance": {
      "storage_type": "Instance",
      "remaining_ttl": 518400,
      "threshold": 518400,
      "bump_amount": 1036800,
      "status": "OK",
      "entries": [
        {
          "contract": "Vault",
          "contract_id": "CDVAULT...",
          "key_desc": "Instance",
          "ttl": 518400,
          "threshold": 518400,
          "bump_amount": 1036800
        }
      ]
    },
    "ProcessedRequest": {
      "storage_type": "Persistent",
      "remaining_ttl": null,
      "threshold": null,
      "bump_amount": null,
      "status": "EMPTY",
      "entries": []
    }
  },
  "errors": []
}
```

`remaining_ttl` for a revenue-pool category is derived from `liveUntilLedgerSeq - latestLedger` as returned by `getLedgerEntries`.

### Exit Codes

- **`0`**: Success (all active categories are above the threshold, no RPC/simulation errors).
- **`1`**: Failure (one or more active categories are below the threshold, or an RPC/simulation error occurred).

---

## Nightly Workflow

The Storage TTL Doctor is configured to run on a nightly cron schedule in `.github/workflows/ttl-doctor.yml`. It runs at 2:00 AM UTC every night, queries the deployed contract addresses configured in GitHub Secrets, and outputs the status report to the action logs.
