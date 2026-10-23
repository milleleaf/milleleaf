#!/bin/bash
# On the build machine (not in a container): the companion workbook's content, then its app saves.
#   WORK=<work folder> IMAGE=<the CI image, with LibreOffice 24.2.7.2> bash make_apps.sh
# LibreOffice Calc in the CI image with no network; gridcraft-cli convert in a confinement of its
# own: no network, every capability dropped, no new privileges, a read-only root, the input read-only,
# only the output directory writable.
set -e
R="${WORK:?set WORK to the work folder}"
IMAGE="${IMAGE:?set IMAGE to the CI image}"
cd $R
DK_NET=none bash tools/dk.sh sh "cd /work/deck && venv/bin/python make_workbook.py milleleaf-charts.openpyxl.xlsx && mkdir -p calc && soffice --headless --norestore --convert-to xlsx:'Calc MS Excel 2007 XML' --outdir calc milleleaf-charts.openpyxl.xlsx >/dev/null 2>&1 && soffice --headless --norestore --convert-to ods --outdir calc milleleaf-charts.openpyxl.xlsx >/dev/null 2>&1 && ls -l calc"
mkdir -p $R/work/deck/gridcraft-in $R/work/deck/gridcraft-out
cp $R/work/deck/milleleaf-charts.openpyxl.xlsx $R/work/deck/gridcraft-in/
docker run --rm --name milleleaf-realswap-gc-$$ --cpus 2 --memory 2g --memory-swap 2g \
  --network none --cap-drop ALL --security-opt no-new-privileges --read-only --tmpfs /tmp:size=64m \
  --user "$(id -u):$(id -g)" \
  -v $R/cache/../cache:/cache:ro -v $R/work/deck/gridcraft-in:/in:ro -v $R/work/deck/gridcraft-out:/out \
  "$IMAGE" /cache/target-gridcraft/debug/gridcraft-cli convert /in/milleleaf-charts.openpyxl.xlsx /out/milleleaf-charts.gridcraft.xlsx
ls -l $R/work/deck/gridcraft-out
