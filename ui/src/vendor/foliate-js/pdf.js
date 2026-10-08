// BiblioChad: PDFs are rendered natively with PDFium, so foliate-js's pdf.js
// adapter (and its bundled pdf.js) is intentionally not vendored.
export const makePDF = async () => {
    throw new Error('PDF rendering is handled by BiblioChad, not foliate-js')
}
