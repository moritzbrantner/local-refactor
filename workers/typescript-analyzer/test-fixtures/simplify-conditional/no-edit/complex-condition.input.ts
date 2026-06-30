export function isReady(value: string | undefined) {
  if (Boolean(value)) {
    return true;
  }
  return false;
}
