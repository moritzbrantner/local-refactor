export function summarizeCart(items: Array<{ price: number }>) {
  const x = items.length;
  const y = items.reduce((total, item) => total + item.price, 0);
  return { itemCount: x, subtotal: y };
}
