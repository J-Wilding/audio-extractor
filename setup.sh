#!/bin/bash
set -e

echo "======================================="
echo "  Ytdlm Audio Extractor Setup Script   "
echo "======================================="

# 1. Check for Homebrew
if ! command -v brew &> /dev/null; then
    echo "❌ Homebrew is not installed. Please install it first from https://brew.sh/"
    exit 1
fi
echo "✅ Homebrew is installed."

# 2. Check for ffmpeg (Required by yt-dlp for audio extraction)
if ! command -v ffmpeg &> /dev/null; then
    echo "⏳ ffmpeg is missing. Installing via Homebrew..."
    brew install ffmpeg
else
    echo "✅ ffmpeg is already installed."
fi

# 3. Check for yt-dlp (Optional system fallback, though our Rust backend manages its own)
if ! command -v yt-dlp &> /dev/null; then
    echo "⏳ yt-dlp is missing globally. Installing via Homebrew just in case..."
    brew install yt-dlp
else
    echo "✅ yt-dlp is already installed globally."
fi

# 4. Install Node dependencies
echo "⏳ Installing NPM dependencies..."
npm install

echo ""
echo "🎉 Setup Complete!"
echo "👉 Run 'npm run tauri dev' to start the application."
