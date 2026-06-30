export type ReportInput = { title: string; total: number };

export function renderReport(input: ReportInput) {
  return `${input.title}: ${input.total}`;
}
