/**
 * Public explorer page for a Cardano transaction. The app's chain writes go
 * to preprod (the bundled network profile), so links do too.
 */
export function explorerTxUrl(txHash: string): string {
  return `https://preprod.cardanoscan.io/transaction/${encodeURIComponent(txHash)}`
}
