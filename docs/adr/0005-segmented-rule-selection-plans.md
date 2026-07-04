# Segmented Rule Selection Plans

Automatic rule selection uses one persisted `Rule Selection Plan` inside a single Run instead of creating child runs per subfolder. This keeps rollback, validation, cancellation, review, and the patch journal unified while still allowing different Target Folder subtrees to receive different Refactoring Rules based on config, file shape, language, and conservative fallbacks.
