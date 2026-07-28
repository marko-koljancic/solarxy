// Parsing an engine cook error back into a position.
//
// Pure and dependency-free so it can be tested without a React tree or the
// wasm client. The engine formats `line N, column M: message`
// (`ExprError::line_col`); this is the only place that shape is decoded, so
// if the engine ever changes it, exactly one thing breaks.

/** The position an engine error points at, if it names one.
 *
 * The engine formats `line N, column M: ...` (`ExprError::line_col`), and
 * both halves are used: the line tints, the column underlines the token.
 * A message with a line but no column still marks the line. */
export function errorPosition(
  message: string | undefined,
): { line: number; column: number; message: string } | null {
  if (!message) return null;
  const line = /\bline (\d+)/.exec(message);
  if (!line) return null;
  const n = Number(line[1]);
  if (!Number.isFinite(n) || n <= 0) return null;
  const col = /\bcolumn (\d+)/.exec(message);
  const c = col ? Number(col[1]) : 1;
  return {
    line: n,
    column: Number.isFinite(c) && c > 0 ? c : 1,
    message,
  };
}

/** The line alone, kept for callers that only tint a row. */
export function errorLine(message: string | undefined): number | null {
  return errorPosition(message)?.line ?? null;
}
