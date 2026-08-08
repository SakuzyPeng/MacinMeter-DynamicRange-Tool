/** Aggregate progress for batch items whose events may arrive out of order. */
export class BatchProgress {
  private total: number;
  private readonly byItem = new Map<number, number>();

  constructor(total: number) {
    this.total = BatchProgress.validTotal(total);
  }

  reset(total: number): void {
    this.total = BatchProgress.validTotal(total);
    this.byItem.clear();
  }

  update(index: number, fraction: number): number {
    if (!Number.isInteger(index) || index < 0 || index >= this.total) {
      return this.percentage();
    }

    const bounded = Number.isFinite(fraction)
      ? Math.max(0, Math.min(1, fraction))
      : 0;
    const previous = this.byItem.get(index) ?? 0;
    this.byItem.set(index, Math.max(previous, bounded));
    return this.percentage();
  }

  private percentage(): number {
    let completed = 0;
    for (const fraction of this.byItem.values()) completed += fraction;
    return (completed / this.total) * 100;
  }

  private static validTotal(total: number): number {
    return Number.isFinite(total) && total > 0 ? Math.floor(total) : 1;
  }
}
