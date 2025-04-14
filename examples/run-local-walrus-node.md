# Walrus Testnet Node Setup Guide

**Important**: If you encounter issues with uploading or downloading while testing the `example`, switch to using a local Walrus node as described in this guide to resolve problems.

This guide walks you through downloading, installing, and running a Walrus testnet node locally, including setting up a Sui testnet wallet, obtaining WAL tokens, and testing the node via REST API.

## References
- [Install Walrus](https://docs.wal.app/usage/setup.html)
- [Run Walrus Publisher & Aggregator](https://docs.wal.app/operator-guide/aggregator.html)
- [Test Walrus REST API](https://docs.wal.app/usage/web-api.html)


## Prerequisites
- A Unix-like system (e.g., macOS or Linux).
- Sui CLI installed (`sui` command available). Install it via `cargo install sui` if needed.
- Basic familiarity with terminal commands.
- A stable internet connection.

## Step 1: Download Walrus Testnet Binary
Download the appropriate Walrus testnet binary for your system:
- For macOS (Apple Silicon): `walrus-testnet-latest-macos-arm64`.
- For Linux or other platforms, check available binaries.

Sources:
- [Official Binary Directory](https://bin.wal.app/)
- [GitHub Releases](https://github.com/MystenLabs/walrus/releases)

Example for macOS:
```bash
curl -O https://bin.wal.app/walrus-testnet-latest-macos-arm64
```

## Step 2: Install Walrus
Set up the downloaded binary to be executable and easily accessible:

```bash
# Create a symbolic link for convenience
ln -s walrus-testnet-latest-macos-arm64 walrus

# Add execute permissions
chmod +x walrus-testnet-latest-macos-arm64
```

Move the binary to a directory in your `PATH` (optional, e.g., `/usr/local/bin`):
```bash
mv walrus /usr/local/bin/
```

## Step 3: Configure Walrus
Download and configure the Walrus client configuration file, which includes package and object IDs for the testnet.

```bash
# Download the config file
curl https://docs.wal.app/setup/client_config.yaml -o ~/.config/walrus/client_config.yaml

# Edit the config to use testnet
sed -i '' 's/default_context: .*/default_context: testnet/' ~/.config/walrus/client_config.yaml
```

Alternatively, edit the file manually with a text editor (e.g., `vi`):
```bash
vi ~/.config/walrus/client_config.yaml
```
Change the `default_context` line to:
```yaml
default_context: testnet
```

## Step 4: Verify Walrus Installation
Check that Walrus is installed correctly and displays the expected version:

```bash
./walrus --version
```

Expected output (version may vary):
```
walrus 1.20.1-20980225ec99
```

## Step 5: Set Up Sui Testnet Wallet
Ensure you have a Sui testnet environment and a wallet with sufficient SUI tokens. Skip this step if you already have a configured wallet.

```bash
# Add testnet environment
sui client new-env --alias testnet --rpc https://fullnode.testnet.sui.io:443

# Switch to testnet
sui client switch --env testnet

# Create a new wallet (e.g., using ed25519 key scheme)
sui client new-address ed25519

# Request SUI tokens from the testnet faucet
sui client faucet
```

Verify your SUI balance:
```bash
sui client balance
```

## Step 6: Obtain WAL Tokens
Exchange SUI for WAL tokens to interact with the Walrus node (default: 0.5 SUI for 0.5 WAL).

```bash
./walrus get-wal
```

Check your wallet balance to confirm WAL tokens:
```bash
sui client balance
```

Expected output (values may vary):
```
╭────────────────────────────────────────────╮
│ Balance of coins owned by this address     │
├────────────────────────────────────────────┤
│ ╭────────────────────────────────────────╮ │
│ │ coin       balance (raw)  balance      │ │
│ ├────────────────────────────────────────┤ │
│ │ Sui        2099026274997  2.09K SUI    │ │
│ │ WAL Token  5996535000     5.99 WAL     │ │
│ ╰────────────────────────────────────────╯ │
╰────────────────────────────────────────────╯
```

**Note**: If you see a client/server API version mismatch warning, ensure your Sui CLI is up-to-date (`cargo install sui`).

## Step 7: Start Walrus Publisher & Aggregator Node
Run a local Walrus node that acts as both a publisher and aggregator. Specify a directory for wallet data (replace `/path/to/wallets` with your preferred location).

```bash
WALLETS_DIR=/path/to/wallets
mkdir -p $WALLETS_DIR

./walrus daemon \
  --bind-address "127.0.0.1:31416" \
  --sub-wallets-dir "$WALLETS_DIR" \
  --n-clients 1
```

This starts the node on `localhost:31416`. Keep the terminal open to keep the node running.

## Step 8: Test the Walrus REST API
Verify the node is operational by interacting with its REST API.

Upload a sample blob:
```bash
curl -X PUT "http://127.0.0.1:31416/v1/blobs" -d "some string"
```

Expected response (details may vary):
```json
{
  "newlyCreated": {
    "blobObject": {
      "id": "0x35a8ed2d52108e0049f486963f4c35b488f58bad64f7ea345aabebc55401a637",
      "registeredEpoch": 11,
      "blobId": "9k95lgtG9iPU8yk_wYz8m8CUY1snveemF8ypfpBLUUg",
      "size": 11,
      "encodingType": "RS2",
      "certifiedEpoch": 11,
      "storage": {
        "id": "0x907c6ca4a8a9f845affa5b93728e22f4b6033c13c75fd68eeca40542fd181da7",
        "startEpoch": 11,
        "endEpoch": 12,
        "storageSize": 66034000
      },
      "deletable": false
    },
    "resourceOperation": {
      "registerFromScratch": {
        "encodedLength": 66034000,
        "epochsAhead": 1
      }
    },
    "cost": 11025000
  }
}
```

## Additional Notes
- **Modifying Node URL or Port**: If you need to change the Walrus node’s URL or port in the `example` directory, search for `localhost` in the relevant files. Update the URL for `publisher7` or `aggregator7` as needed to match your configuration (e.g., change `127.0.0.1:31416` to your desired address or port).
- **Version Compatibility**: Ensure all dependencies (e.g., Sui CLI, Walrus binary) are up-to-date to avoid mismatches.
- **Local Testing**: Always test changes locally before deploying to ensure the node and API function correctly.

## Troubleshooting
- **Binary not found**: Ensure the `walrus` binary is in your `PATH` or use `./walrus` if it’s in the current directory.
- **Config file issues**: Verify `~/.config/walrus/client_config.yaml` exists and has `default_context: testnet`.
- **Sui CLI errors**: Update Sui CLI to the latest version if you encounter API mismatches.
- **Node not starting**: Check that port `31416` is free and `WALLETS_DIR` is writable.
- **API test fails**: Confirm the node is running and accessible at `127.0.0.1:31416`.
- **Upload/Download issues in `example`**: Ensure you’re using a local Walrus node (set up in Step 7) and verify the node URL/port in the `example` configuration.


