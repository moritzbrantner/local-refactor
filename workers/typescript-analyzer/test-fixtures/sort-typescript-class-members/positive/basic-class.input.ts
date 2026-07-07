export class CartSummary {
  total() {
    return this.count;
  }

  static label: string;

  constructor(private readonly count: number) {}

  readonly id: string;

  describe() {
    return `${this.id}:${this.count}`;
  }
}
