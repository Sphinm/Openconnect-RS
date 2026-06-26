#!/bin/sh
# Collect CI artifacts locally (optional). Tag releases are published automatically by
# .github/workflows/release.yml via softprops/action-gh-release.

CURRENT_DIR=$(pwd)

download() {
    if [ -d "./artifacts" ]; then
        echo "The artifacts directory already exists, please remove it first."
        exit 1
    fi

    echo "Downloading the artifacts..."
    mkdir -p ./artifacts
    # # download the artifact by the following commands, select the all openconnect-xx artifacts
    gh run download --dir ./artifacts
    echo ""

    echo "Renaming the CLI binaries..."
    if [ -d "./artifacts/openconnect-linux-x64" ]; then
        mv ./artifacts/openconnect-linux-x64/openconnect-cli ./artifacts/openconnect-linux-x64/openconnect-cli_linux-x86_64
    fi

    if [ -d "./artifacts/openconnect-mac-aarch64" ]; then
        mv ./artifacts/openconnect-mac-aarch64/openconnect-cli ./artifacts/openconnect-mac-aarch64/openconnect-cli_osx-aarch64
    fi

    if [ -d "./artifacts/openconnect-mac-x64" ]; then
        mv ./artifacts/openconnect-mac-x64/openconnect-cli ./artifacts/openconnect-mac-x64/openconnect-cli_osx-x86_64
    fi
    echo ""

    echo "Renaming the GUI binaries..."
    if [ -d "./artifacts/openconnect-win" ]; then
        if [ -f "./artifacts/openconnect-win/native/openconnect-gui-win.exe" ]; then
            cp ./artifacts/openconnect-win/native/openconnect-gui-win.exe \
                ./artifacts/openconnect-win/openconnect-gui-win-x86_64.exe
        fi
        if [ -d "./artifacts/openconnect-win/nsis" ]; then
            mv ./artifacts/openconnect-win/msi/*.msi ./artifacts/openconnect-win/msi/openconnect-gui_win-x86_64.msi 2>/dev/null || true
            mv ./artifacts/openconnect-win/nsis/*.exe ./artifacts/openconnect-win/nsis/openconnect-gui_win-x86_64.exe 2>/dev/null || true
        fi
    fi
    echo ""

    if [[ "$OSTYPE" = "darwin"* ]]; then
        # codesign the macos bundle
        CODESIGN_IDENTITY=$(security find-identity -p codesigning | grep "CSSMERR_TP_NOT_TRUSTED" | awk '{print $3}' | tr -d '"')

        # process the macos aarch64 bundle
        echo "Codesigning the macos aarch64 bundle..."

        cd ./artifacts/openconnect-mac-aarch64/bundle/macos
        chmod +x ./openconnect-gui.app/Contents/MacOS/openconnect-gui
        codesign -fs "$CODESIGN_IDENTITY" ./openconnect-gui.app
        echo ""

        echo "Creating the aarch 64 dmg files..."
        # create dmg
        create-dmg \
            --volname "Openconnect GUI" \
            --window-pos 200 120 \
            --window-size 800 400 \
            --icon-size 100 \
            --icon "openconnect-gui.app" 200 190 \
            --hide-extension "openconnect-gui.app" \
            --app-drop-link 600 185 \
            "openconnect-gui_osx-aarch64.dmg" \
            "openconnect-gui.app/"

        cd $CURRENT_DIR
        echo ""

        # process the macos x86_64 bundle
        echo "Codesigning the macos x86_64 bundle..."

        cd ./artifacts/openconnect-mac-x64/bundle/macos
        chmod +x ./openconnect-gui.app/Contents/MacOS/openconnect-gui
        codesign -fs "$CODESIGN_IDENTITY" ./openconnect-gui.app
        echo ""

        echo "Creating the x86_64 dmg files..."
        create-dmg \
            --volname "Openconnect GUI" \
            --window-pos 200 120 \
            --window-size 800 400 \
            --icon-size 100 \
            --icon "openconnect-gui.app" 200 190 \
            --hide-extension "openconnect-gui.app" \
            --app-drop-link 600 185 \
            "openconnect-gui_osx-x86_64.dmg" \
            "openconnect-gui.app/"

        cd $CURRENT_DIR
        echo ""
    fi
}

clean() {
    echo "Cleaning the artifacts..."
    rm -rf ./artifacts
}

if [ "$1" = "download" ]; then
    download
elif [ "$1" = "clean" ]; then
    clean
else
    echo "Usage: $0 {download|clean}"
    exit 1
fi
