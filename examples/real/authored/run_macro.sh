#!/bin/bash
# Inside the CI image (LibreOffice 24.2.7.2): install Module1 as the user's Standard Basic library, then run it headless.
set -e
cd /work/deck
soffice --headless --norestore --terminate_after_init >/dev/null 2>&1 || true
B=$HOME/.config/libreoffice/4/user/basic/Standard
mkdir -p "$B"
cp Module1.xba "$B/Module1.xba"
cat > "$B/script.xlb" <<'X'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE library:library PUBLIC "-//OpenOffice.org//DTD OfficeDocument 1.0//EN" "library.dtd">
<library:library xmlns:library="http://openoffice.org/2000/library" library:name="Standard" library:readonly="false" library:passwordprotected="false">
 <library:element library:name="Module1"/>
</library:library>
X
mkdir -p impress
DECK_OUT=/work/deck/impress timeout 120 soffice --headless --invisible --norestore "macro:///Standard.Module1.Main" 2>&1 | tail -5 || true
ls -l impress
