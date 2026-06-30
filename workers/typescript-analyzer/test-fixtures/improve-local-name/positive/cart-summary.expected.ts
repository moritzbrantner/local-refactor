export function summarizeCart(items: Array<{ price: number }>) {
  const itemCount = items.length;
  const subtotal = items.reduce((total, item) => total + item.price, 0);
  return { itemCount, subtotal };
}
