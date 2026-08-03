# Data format

The default generalized schema requires:

| Concept | Default column | Rule |
|---|---|---|
| Jurisdiction | `Jurisdiction` | Nonempty identifier |
| Precinct | `Precinct` | Nonempty identifier |
| Valid contest votes | `Valid_Contest_Votes` | Nonnegative integer |
| Candidate A | `Votes_Candidate_A` | Nonnegative integer |
| Candidate B | `Votes_Candidate_B` | Nonnegative integer |

Optional mappings include registered voters, active registered voters, ballots cast, write-ins, undervotes, overvotes, latitude, longitude, reported turnout, vote type, and down-ballot candidates. Candidate definitions are an arbitrary nonempty list; the A/B names are defaults, not a two-candidate restriction.

All input columns are preserved. Internally generated values do not overwrite the source map.

The legacy adapter recognizes `County`, `Precinct`, `Registered_Dem`, `Registered_Rep`, `Votes_Harris`, `Votes_Trump`, `Total_Votes`, and `Turnout_Percent`. It explicitly derives registration as Democratic plus Republican registration and emits a warning because that assumption is not portable.
