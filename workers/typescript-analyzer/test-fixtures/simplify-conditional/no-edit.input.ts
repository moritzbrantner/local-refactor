export function isReady(value: boolean, override: boolean) {
  if (override) {
    return value;
  }
  return false;
}
