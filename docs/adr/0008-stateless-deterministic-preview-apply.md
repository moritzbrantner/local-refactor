# Stateless Deterministic Preview Apply

Deterministic Previews are stateless and are not persisted. Applying a preview recomputes the deterministic edits on the server, compares the preview fingerprint, and only then creates a normal Run that writes through the patch journal and validation lifecycle. This preserves review trust without adding durable preview lifecycle, stale-preview storage, or a weaker service boundary that trusts client-supplied edits.
