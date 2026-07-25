Status: needs-triage
Type: feature

# Metadata Preview Fetching (v2)

For the P0 prototype, clicking "Extract Audio" simply says "Downloading..." while the engine handles everything.

For a future iteration, the app should eagerly fetch the metadata of the inputted URL before executing the full download. The UI should dynamically update to say "Fetching [Album Name]..." or "Fetching [Song Name]..." so the user gets immediate visual confirmation that the correct media was identified.

This will likely involve querying `yt-dlp --dump-json` in the background before kicking off the actual download, or parsing the stdout of the extraction process early.
