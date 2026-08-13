# Use a coordinator with bounded background workers

`argos-explorer` keeps application state, input handling, and rendering on one coordinator thread while bounded workers perform filesystem, Git, indexing, decoding, and search work. This was chosen over a Tokio runtime because these workloads are predominantly blocking; typed channels, generation identifiers, cooperative cancellation, and latest-request-wins result handling keep the UI responsive without mixing async and blocking execution models.
