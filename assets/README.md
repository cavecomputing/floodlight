# assets

`oui.csv` is the IEEE's MA-L registry (https://standards-oui.ieee.org/oui/oui.csv), cut down to
the 24-bit prefix and the organisation's name, and built into the binary. To refresh it:

```bash
curl -fsSL https://standards-oui.ieee.org/oui/oui.csv -o /tmp/oui.csv
python3 -c '
import csv
rows = csv.reader(open("/tmp/oui.csv", newline="", encoding="utf-8")); next(rows)
w = csv.writer(open("assets/oui.csv", "w", newline="", encoding="utf-8"), lineterminator="\n")
w.writerow(["prefix", "name"])
[w.writerow([r[1].lower(), r[2].strip()]) for r in rows if r[0] == "MA-L"]'
```
