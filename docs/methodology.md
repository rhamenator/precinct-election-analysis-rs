# Methodology and limitations

This system follows three rules: preserve official inputs, state every denominator, and report methods independently.

## Turnout/share residuals

Turnout is calculated from ballots and registration when both are available; otherwise the explicitly mapped reported-turnout field is used. Candidate share is candidate votes divided by valid votes in the selected contest. A linear baseline is fit using observations at or below the configured turnout quantile. Reference observations use leave-one-out residual adjustment. A large studentized residual is unusual under this model, not evidence of a cause.

## Vote share by count and down-ballot difference

These are descriptive. The former reports a linear trend and residual without an anomaly flag. The latter implements `100 × (presidential − down-ballot) / presidential` for documented same-party pairs. Roll-off, split-ticket voting, eligibility, and candidate effects matter.

## Digits

Last digits are compared with a uniform distribution using a chi-square goodness-of-fit test. Holm correction controls family-wise error across successful candidate tests. This is a dataset-level diagnostic; it is not attached to each precinct as a score. Benford analysis is intentionally absent because ordinary precinct datasets often fail its preconditions.

## Spatial diagnostics

The current implementation uses row-standardized K-nearest-neighbor weights over valid coordinate pairs and labels that choice as a fallback. Global and local Moran statistics use reproducible permutations. Local p-values receive Benjamini–Hochberg correction. Geographic political clustering is an ordinary explanation for spatial association.

## Robust multivariate diagnostic

The Rust version intentionally avoids a third-party Isolation Forest implementation that cannot meet this project’s deterministic and normalization requirements. It uses median/MAD standardized distances over a fixed numeric feature allow-list, caps individual robust z values, and maps the mean distance to `[0, 1]` with `1 − exp(−z/3)`. The configured threshold is a review convention, not a calibrated probability.

## Known limits

- Aggregate totals cannot establish voter intent or ballot-level correctness.
- The web UI presents structured results but does not yet render geographic polygons.
- KNN uses coordinate degrees, not a projected distance metric; use a jurisdiction-appropriate spatial workflow for publication-grade geography.
- Synthetic tests establish invariants and known-case behavior, not universal sensitivity or specificity.
- The application does not retrieve official election data or contextual covariates automatically.
