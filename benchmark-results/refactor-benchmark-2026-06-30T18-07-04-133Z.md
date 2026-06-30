# Refactoring Benchmark

Generated: 2026-06-30T18:07:33.846Z

Model: qwen2.5-coder:7b

## Summary

- Rules: 12
- Succeeded: 12
- Failed: 0
- Total wall time: 23826 ms
- Average wall time: 1986 ms
- Slowest rule: extract-duplicate-block

## Results

rule | status | wallMs | diffFiles | servicePeakRssMb | ollamaPeakRssMb | nvidiaPeakTotalVramMb | nvidiaPeakOllamaVramMb
--- | --- | ---: | ---: | ---: | ---: | ---: | ---:
simplify-conditional | succeeded | 646 | 1 | 19.1 | 30.8 | 1076 | 
convert-nested-if-to-guard-clause | succeeded | 651 | 1 | 19.2 | 30.1 | 1067 | 
extract-type-definition | succeeded | 644 | 1 | 19.2 | 30.1 | 1067 | 
inline-trivial-helper | succeeded | 647 | 1 | 19.2 | 30.1 | 1067 | 
normalize-imports | succeeded | 648 | 1 | 19.3 | 30.1 | 1067 | 
sort-independent-declarations | succeeded | 639 | 1 | 19.3 | 30.1 | 1067 | 
improve-local-name | succeeded | 636 | 1 | 19.3 | 30.1 | 1071 | 
extract-duplicate-block | succeeded | 6245 | 1 | 19.4 | 4825.8 | 5823 | 4694
split-oversized-function | succeeded | 4136 | 1 | 19.5 | 782.9 | 5814 | 4694
isolate-side-effect-free-helper | succeeded | 3383 | 1 | 19.6 | 814.2 | 5797 | 4694
split-file-by-responsibility | succeeded | 3147 | 2 | 19.6 | 843.3 | 5792 | 4694
extract-parameter-object | succeeded | 2404 | 1 | 19.6 | 868.3 | 5774 | 4694

## Notes

- wallMs measures POST /api/runs until terminal run status.
- servicePeakRssMb samples the local-refactor-service process RSS from /proc.
- ollamaPeakRssMb sums RSS for processes whose command contains ollama.
- nvidiaPeak* fields use nvidia-smi when available; null means unavailable or no matching process.
- Runs use validationCommands: ["true"] to measure refactoring overhead without project test cost.
