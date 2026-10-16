// Excel (.xlsx) and PDF export, sharing the same {headers, rows} shape the CSV
// exporter uses. Local dev app (not an artifact), so downloads work normally.
import * as XLSX from "xlsx";
import { jsPDF } from "jspdf";
import autoTable from "jspdf-autotable";

export function downloadXlsx(filename, headers, rows, sheetName = "Report") {
  const ws = XLSX.utils.aoa_to_sheet([headers, ...rows]);
  // Auto-fit columns to the widest cell (capped).
  ws["!cols"] = headers.map((h, i) => {
    const widest = Math.max(String(h).length, ...rows.map((r) => String(r[i] ?? "").length));
    return { wch: Math.min(widest + 2, 60) };
  });
  const wb = XLSX.utils.book_new();
  XLSX.utils.book_append_sheet(wb, ws, sheetName);
  XLSX.writeFile(wb, filename);
}

export function downloadPdf(filename, title, headers, rows) {
  const landscape = headers.length > 6;
  const doc = new jsPDF({ orientation: landscape ? "landscape" : "portrait" });

  doc.setFont("helvetica", "bold");
  doc.setFontSize(16);
  doc.setTextColor(236, 106, 44); // Sankayi orange
  doc.text("Sankayi Jazz Series", 14, 18);

  doc.setFont("helvetica", "normal");
  doc.setFontSize(11);
  doc.setTextColor(40, 40, 40);
  doc.text(title, 14, 26);

  doc.setFontSize(8);
  doc.setTextColor(140, 140, 140);
  doc.text(`Generated ${new Date().toLocaleString("en-GB")} · ${rows.length} rows`, 14, 31);

  autoTable(doc, {
    head: [headers],
    body: rows.map((r) => r.map((c) => (c == null ? "" : String(c)))),
    startY: 36,
    styles: { fontSize: 8, cellPadding: 2.5, overflow: "linebreak" },
    headStyles: { fillColor: [236, 106, 44], textColor: 255, fontStyle: "bold" },
    alternateRowStyles: { fillColor: [246, 244, 250] },
    margin: { left: 14, right: 14 },
  });

  doc.save(filename);
}
