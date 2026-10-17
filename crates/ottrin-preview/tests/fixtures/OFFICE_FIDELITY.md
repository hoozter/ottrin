# Owned Office layout fixtures

`generate-office-fidelity.py` authors these three fixtures with python-docx 1.2.0,
openpyxl 3.1.5, python-pptx 1.0.2 and Pillow 12.3.0. These are authoring tools;
Ottrin does not depend on them. The generator fixes document metadata and ZIP
entry order/timestamps. Regenerating twice must produce identical SHA-256 hashes.
All content and the two-color bitmap are generated here; there are no external
images, macros, links or user documents.

- `office-fidelity.docx`: US Letter, one-inch horizontal margins; centered blue
  bold 20-point Arial heading, bold and italic body runs, an orange/cyan image,
  a bordered two-column table, then an explicit second page with right-aligned
  text. This intentionally uses valid run properties rather than relying on
  text-only extraction to imply formatting.
- `office-fidelity.xlsx`: landscape A4, a merged blue heading with white text,
  a bordered budget table with currency formats, calculated line amounts and
  a `$4,000.00` total, and a green bar chart. The print area fits on one page.
- `office-fidelity.pptx`: two 10-by-5.625-inch slides with centered blue bold
  24-point headings. The first has a green rounded shape and orange/cyan image;
  the second has its own body text, making page order and separation visible.

The ignored `native_installed_office_preserves_representative_layout` test runs
these through the production isolated provider. It checks page count/size,
selected text positions, type size/font/color, formulas, row order and raster
colors. It exports the PDFs and all five 1200-pixel page images to a new owned
`OTTRIN_NATIVE_OFFICE_FIDELITY_OUTPUT` directory for visual review. Original
bytes and private-state cleanup are checked independently. The QA actor owns
and removes that output after collecting its compact evidence.

These fixtures cover representative ordinary layouts, not arbitrary Office
compatibility: complex scripts, tracked changes, advanced formulas, animations,
OLE objects and missing-font substitution need separate evidence.
