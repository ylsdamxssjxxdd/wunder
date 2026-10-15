// AI生成
// 舰桥导出用的极简 XLSX 写出：单工作表、纯文本与数值单元格、ZIP 打包。
// 不引入第三方库，浏览器端直接生成可被 Excel / WPS / LibreOffice 打开的 .xlsx。

const CRC_TABLE = (() => {
  const table = new Uint32Array(256);
  for (let index = 0; index < 256; index += 1) {
    let value = index;
    for (let bit = 0; bit < 8; bit += 1) {
      value = value & 1 ? 0xedb88320 ^ (value >>> 1) : value >>> 1;
    }
    table[index] = value >>> 0;
  }
  return table;
})();

const crc32 = (bytes) => {
  let crc = 0xffffffff;
  for (let index = 0; index < bytes.length; index += 1) {
    crc = CRC_TABLE[(crc ^ bytes[index]) & 0xff] ^ (crc >>> 8);
  }
  return (crc ^ 0xffffffff) >>> 0;
};

const encodeUtf8 = (text) => new TextEncoder().encode(String(text ?? ""));

const escapeXml = (value) =>
  String(value ?? "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    // 控制字符在 XML 1.0 中不合法，统一剔除避免生成损坏文件
    .replace(/[\x00-\x08\x0b\x0c\x0e-\x1f]/g, "");

const columnName = (index) => {
  let name = "";
  let value = index;
  while (value >= 0) {
    name = String.fromCharCode((value % 26) + 65) + name;
    value = Math.floor(value / 26) - 1;
  }
  return name;
};

const cellXml = (rowIndex, columnIndex, value) => {
  const ref = `${columnName(columnIndex)}${rowIndex}`;
  if (value === null || value === undefined || value === "") {
    return "";
  }
  if (typeof value === "number" && Number.isFinite(value)) {
    return `<c r="${ref}"><v>${value}</v></c>`;
  }
  return `<c r="${ref}" t="inlineStr"><is><t xml:space="preserve">${escapeXml(
    value
  )}</t></is></c>`;
};

const buildSheetXml = (rows) => {
  const body = rows
    .map((row, rowOffset) => {
      const rowIndex = rowOffset + 1;
      const cells = row
        .map((value, columnIndex) => cellXml(rowIndex, columnIndex, value))
        .join("");
      return `<row r="${rowIndex}">${cells}</row>`;
    })
    .join("");
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>${body}</sheetData></worksheet>`;
};

const buildPackageFiles = (sheetName, rows) => {
  const safeSheetName = String(sheetName || "Sheet1")
    .replace(/[\\\/\?\*\[\]:]/g, "_")
    .slice(0, 31);
  return [
    {
      name: "[Content_Types].xml",
      data: `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>`,
    },
    {
      name: "_rels/.rels",
      data: `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>`,
    },
    {
      name: "xl/workbook.xml",
      data: `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="${escapeXml(
        safeSheetName
      )}" sheetId="1" r:id="rId1"/></sheets></workbook>`,
    },
    {
      name: "xl/_rels/workbook.xml.rels",
      data: `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>`,
    },
    { name: "xl/worksheets/sheet1.xml", data: buildSheetXml(rows) },
  ];
};

// 优先用 deflate-raw 压缩；不支持时退化为 store，两种写法 Excel 都能识别
const compressEntry = async (bytes) => {
  if (typeof CompressionStream === "undefined") {
    return { method: 0, data: bytes };
  }
  try {
    const stream = new Blob([bytes])
      .stream()
      .pipeThrough(new CompressionStream("deflate-raw"));
    const buffer = await new Response(stream).arrayBuffer();
    const compressed = new Uint8Array(buffer);
    if (compressed.length < bytes.length) {
      return { method: 8, data: compressed };
    }
    return { method: 0, data: bytes };
  } catch {
    return { method: 0, data: bytes };
  }
};

// ZIP 打包：local header + 数据 + 中央目录 + EOCD
const zipFiles = async (files) => {
  const prepared = await Promise.all(
    files.map(async (file) => {
      const raw = encodeUtf8(file.data);
      const packed = await compressEntry(raw);
      return {
        nameBytes: encodeUtf8(file.name),
        method: packed.method,
        data: packed.data,
        rawSize: raw.length,
        crc: crc32(raw),
      };
    })
  );

  const chunks = [];
  const central = [];
  let offset = 0;
  // 固定 DOS 时间戳（2020-01-01 00:00:00），避免依赖运行时时钟造成输出不稳定
  const dosTime = 0;
  const dosDate = 0x5021;

  const writeUint16 = (view, position, value) => view.setUint16(position, value, true);
  const writeUint32 = (view, position, value) => view.setUint32(position, value, true);

  prepared.forEach((entry) => {
    const localHeader = new ArrayBuffer(30 + entry.nameBytes.length);
    const localView = new DataView(localHeader);
    writeUint32(localView, 0, 0x04034b50);
    writeUint16(localView, 4, 20);
    writeUint16(localView, 6, 0);
    writeUint16(localView, 8, entry.method);
    writeUint16(localView, 10, dosTime);
    writeUint16(localView, 12, dosDate);
    writeUint32(localView, 14, entry.crc);
    writeUint32(localView, 18, entry.data.length);
    writeUint32(localView, 22, entry.rawSize);
    writeUint16(localView, 26, entry.nameBytes.length);
    writeUint16(localView, 28, 0);
    const localBytes = new Uint8Array(localHeader);
    localBytes.set(entry.nameBytes, 30);

    const centralHeader = new ArrayBuffer(46 + entry.nameBytes.length);
    const centralView = new DataView(centralHeader);
    writeUint32(centralView, 0, 0x02014b50);
    writeUint16(centralView, 4, 20);
    writeUint16(centralView, 6, 20);
    writeUint16(centralView, 8, 0);
    writeUint16(centralView, 10, entry.method);
    writeUint16(centralView, 12, dosTime);
    writeUint16(centralView, 14, dosDate);
    writeUint32(centralView, 16, entry.crc);
    writeUint32(centralView, 20, entry.data.length);
    writeUint32(centralView, 24, entry.rawSize);
    writeUint16(centralView, 28, entry.nameBytes.length);
    writeUint16(centralView, 30, 0);
    writeUint16(centralView, 32, 0);
    writeUint16(centralView, 34, 0);
    writeUint16(centralView, 36, 0);
    writeUint32(centralView, 38, 0);
    writeUint32(centralView, 42, offset);
    const centralBytes = new Uint8Array(centralHeader);
    centralBytes.set(entry.nameBytes, 46);

    chunks.push(localBytes, entry.data);
    central.push(centralBytes);
    offset += localBytes.length + entry.data.length;
  });

  const centralSize = central.reduce((sum, item) => sum + item.length, 0);
  const endRecord = new ArrayBuffer(22);
  const endView = new DataView(endRecord);
  writeUint32(endView, 0, 0x06054b50);
  writeUint16(endView, 4, 0);
  writeUint16(endView, 6, 0);
  writeUint16(endView, 8, prepared.length);
  writeUint16(endView, 10, prepared.length);
  writeUint32(endView, 12, centralSize);
  writeUint32(endView, 16, offset);
  writeUint16(endView, 20, 0);

  return new Blob([...chunks, ...central, new Uint8Array(endRecord)], {
    type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
  });
};

export const buildXlsxBlob = async ({ sheetName = "Sheet1", rows = [] } = {}) => {
  const safeRows = Array.isArray(rows) ? rows : [];
  return zipFiles(buildPackageFiles(sheetName, safeRows));
};

export const downloadBlob = (blob, filename) => {
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  document.body.appendChild(link);
  link.click();
  link.remove();
  // 立即回收会让部分浏览器拿不到数据，延后释放
  setTimeout(() => URL.revokeObjectURL(url), 2000);
};

export const downloadXlsx = async ({ filename, sheetName, rows }) => {
  const blob = await buildXlsxBlob({ sheetName, rows });
  downloadBlob(blob, filename);
};
