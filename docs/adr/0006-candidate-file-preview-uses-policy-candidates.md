# Candidate File Preview Uses Policy Candidates

Candidate File Preview shows policy-aligned candidate files before a Run is created, not predicted model or analyzer edits.

This is less exact than a dry run, but it is fast, deterministic, avoids model downloads and planning before confirmation, and matches the service's write-safety boundary.

Rejected alternatives:

- Predict edits before confirmation.
- Return full uncapped lists for every Run.
- Block broad Runs until the user narrows the Target Folder.
