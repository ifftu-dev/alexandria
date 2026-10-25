/**
 * Marks the unreachable arm of a switch over a closed union. When a value
 * is added to the union (for the generated vocabularies, in Rust), every
 * switch that does not handle it stops compiling here.
 */
export function assertNever(value: never): never {
  throw new Error(`Unhandled case: ${JSON.stringify(value)}`)
}
