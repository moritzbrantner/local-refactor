export class CartSummary {
  static label: string;

  readonly id: string;

  constructor(private readonly count: number) {}

  describe() {
    return `${this.id}:${this.count}`;
  }

  total() {
    return this.count;
  }
}
