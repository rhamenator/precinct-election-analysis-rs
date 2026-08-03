# Stepwise user guide

## 1. Install and build

1. Install Rust 1.89 or newer with `rustup`.
2. Clone the repository.
3. Run `cargo build --release` from the repository root.
4. Keep the resulting executable and `config.yaml` together, or pass an absolute configuration path.

## 2. Prepare data

1. Use precinct totals from an authoritative election source.
2. Retain the source URL, publication date, election definition, and any notes about vote types or reporting units outside this application.
3. Make a working copy of `config.yaml`.
4. Map the jurisdiction, precinct, candidates, and valid-contest-vote denominator. Map ballots, registration, reported turnout, coordinates, and down-ballot contests only when their definitions are known.
5. If multiple rows share a precinct because they represent mail, early, election-day, or provisional votes, map `vote_type` so it becomes part of the stable identifier.

## 3. Validate

```powershell
precinct-election-analysis-rs.exe --config config.yaml validate official.csv > validation.json
```

1. Confirm `source_schema` is `configured` unless you intentionally supplied the old legacy format.
2. Compare `original_rows`, `accepted_rows`, and `excluded_rows`.
3. Review all error codes. Excluded source rows are preserved; the software does not repair official totals.
4. Review coordinate and turnout warnings even when no row is excluded.

## 4. Analyze and export

```powershell
precinct-election-analysis-rs.exe --config config.yaml analyze official.csv `
  --candidate candidate_a `
  --methods turnout_share,vote_share_by_count,down_ballot_difference,digit_diagnostics,spatial,robust_multivariate `
  --output analysis.zip
```

1. Open `run.json` and verify the candidate, schema, seed, requested methods, and method statuses.
2. Do not treat unavailable methods as successful or infer their results.
3. Use `analysis_results.csv` to identify records associated with a named method.
4. Use `excluded_records.json` to reconcile validation failures with the source.
5. Read `report.md` before sharing results.

## 5. Use the web application

1. Run `precinct-election-analysis-rs.exe --config config.yaml serve`.
2. Open `http://127.0.0.1:8080`.
3. Download the fictional sample if you want to test the workflow.
4. Choose a CSV, enter a configured candidate key, and run the analysis.
5. Review the JSON method statuses and diagnostics. The browser uses the same backend as the CLI.

## 6. Connect an LLM through MCP

1. Build the release executable.
2. Add the MCP configuration shown in the README to your MCP client.
3. Restart the client and call `health`.
4. Call `validate_csv` before `analyze_csv`.
5. Use `narrative_context` when requesting prose. Require the model to preserve every method status and interpretation limitation.
6. Never provide API credentials inside election CSV data or MCP arguments.

## 7. Interpret responsibly

A flag is conditional on its model, configuration, and available aggregate data. Possible ordinary explanations include reporting definitions, geography, demographics, vote method, contest eligibility, candidate effects, precinct size, and data-entry conventions. Investigate source data and context. Use a properly designed ballot audit for ballot evidence.
