import { describe, it, expect } from "vitest";
import {
  bandRows,
  clipToColumn,
  groupRows,
  mergeSpans,
  piecesForLine,
  replaceBoxes,
  type Box,
  type LineMeasure,
} from "../../editor/selection-geometry";

// A line box at y 100..124 in a column from x 100 to x 500, glyph boxes 18px
// tall inside 24px rows, as prose text sits in its line height.
const COLUMN = { left: 100, right: 500 };
const SPACE = 4;

function glyphs(left: number, right: number, row = 0): Box {
  return { left, right, top: 103 + row * 24, bottom: 121 + row * 24 };
}

function line(overrides: Partial<LineMeasure>): LineMeasure {
  return {
    selected: [],
    unselected: [],
    content: { left: 106, right: 498, top: 100, bottom: 124 },
    rtl: false,
    wraps: true,
    newline: false,
    newlineWidth: SPACE,
    emptyStart: null,
    ...overrides,
  };
}

describe("piecesForLine", () => {
  it("covers the selected characters at the line's full height, not the column", () => {
    const pieces = piecesForLine(line({ selected: [glyphs(150, 260)], unselected: [glyphs(106, 150)] }), COLUMN);
    expect(pieces).toEqual([{ left: 150, right: 260, top: 100, bottom: 124 }]);
  });

  it("adds a newline box one space wide after the last character when the line break is selected", () => {
    const pieces = piecesForLine(line({ selected: [glyphs(106, 260)], newline: true }), COLUMN);
    expect(pieces).toEqual([{ left: 106, right: 260 + SPACE, top: 100, bottom: 124 }]);
  });

  it("puts the newline box at the left end of a right-to-left line", () => {
    const pieces = piecesForLine(line({ selected: [glyphs(300, 498)], newline: true, rtl: true }), COLUMN);
    expect(pieces).toEqual([{ left: 300 - SPACE, right: 498, top: 100, bottom: 124 }]);
  });

  it("paints an empty selected line as a newline box at its start", () => {
    expect(piecesForLine(line({ newline: true, emptyStart: 106 }), COLUMN)).toEqual([
      { left: 106, right: 106 + SPACE, top: 100, bottom: 124 },
    ]);
    expect(piecesForLine(line({ newline: true, rtl: true }), COLUMN)).toEqual([
      { left: 498 - SPACE, right: 498, top: 100, bottom: 124 },
    ]);
  });

  it("paints nothing on an empty line whose line break is outside the selection", () => {
    expect(piecesForLine(line({}), COLUMN)).toEqual([]);
  });

  it("paints one piece per wrapped row, from the selection start to the row's last character", () => {
    const content = { left: 106, right: 498, top: 100, bottom: 172 };
    const pieces = piecesForLine(
      line({
        content,
        selected: [glyphs(300, 470, 0), glyphs(106, 490, 1), glyphs(106, 200, 2)],
        unselected: [glyphs(106, 300, 0), glyphs(200, 330, 2)],
      }),
      COLUMN,
    );
    expect(pieces).toEqual([
      { left: 300, right: 470, top: 100, bottom: 124 },
      { left: 106, right: 490, top: 124, bottom: 148 },
      { left: 106, right: 200, top: 148, bottom: 172 },
    ]);
  });

  it("leaves no gap between the rows of one line", () => {
    const content = { left: 106, right: 498, top: 100, bottom: 148 };
    const [first, second] = piecesForLine(
      line({ content, selected: [glyphs(106, 400, 0), glyphs(106, 300, 1)] }),
      COLUMN,
    );
    expect(first.bottom).toBe(second.top);
    expect(first.top).toBe(content.top);
    expect(second.bottom).toBe(content.bottom);
  });

  it("never paints into the column's gutters", () => {
    const pieces = piecesForLine(
      line({ selected: [{ left: 80, right: 520, top: 103, bottom: 121 }], newline: true }),
      COLUMN,
    );
    expect(pieces).toEqual([{ left: 100, right: 500, top: 100, bottom: 124 }]);
  });

  it("keeps the newline box inside the line's padding", () => {
    const pieces = piecesForLine(line({ selected: [glyphs(106, 497)], newline: true }), COLUMN);
    expect(pieces).toEqual([{ left: 106, right: 498, top: 100, bottom: 124 }]);
    expect(piecesForLine(line({ selected: [glyphs(106, 497)], newline: true, wraps: false }), COLUMN)).toEqual(
      pieces,
    );
  });

  it("follows a line that runs past the column when wrapping is off, newline box included", () => {
    const pieces = piecesForLine(
      line({ selected: [glyphs(300, 900)], unselected: [glyphs(106, 300)], newline: true, wraps: false }),
      COLUMN,
    );
    expect(pieces).toEqual([{ left: 300, right: 900 + SPACE, top: 100, bottom: 124 }]);
  });

  it("paints the visible part of a long line scrolled past the column's start", () => {
    const column = { left: -1076.8, right: -443.2 };
    const content = { left: -1070.8, right: -445.2, top: 100, bottom: 124 };
    const pieces = piecesForLine(
      line({ content, selected: [glyphs(-1030.6, 1278.9)], unselected: [glyphs(-1070.8, -1030.6)], wraps: false }),
      column,
    );
    expect(pieces).toEqual([{ left: -1030.6, right: 1278.9, top: 100, bottom: 124 }]);
  });

  it("puts the newline box before the start of a right-to-left line that runs past the column", () => {
    const pieces = piecesForLine(
      line({ selected: [glyphs(-200, 498)], newline: true, rtl: true, wraps: false }),
      COLUMN,
    );
    expect(pieces).toEqual([{ left: -200 - SPACE, right: 498, top: 100, bottom: 124 }]);
  });

  it("keeps the two visual pieces of a selection that bidi reordering splits", () => {
    const pieces = piecesForLine(
      line({ selected: [glyphs(106, 180), glyphs(260, 320)], unselected: [glyphs(180, 260)] }),
      COLUMN,
    );
    expect(pieces).toEqual([
      { left: 106, right: 180, top: 100, bottom: 124 },
      { left: 260, right: 320, top: 100, bottom: 124 },
    ]);
  });

  it("ignores boxes too narrow to paint", () => {
    const buffer = { left: 300, right: 300, top: 100, bottom: 118 };
    expect(piecesForLine(line({ selected: [glyphs(106, 200), buffer] }), COLUMN)).toEqual([
      { left: 106, right: 200, top: 100, bottom: 124 },
    ]);
  });
});

describe("groupRows", () => {
  it("keeps text of mixed sizes on one row together", () => {
    const rows = groupRows([
      { box: glyphs(106, 200), selected: true },
      { box: { left: 200, right: 260, top: 105, bottom: 119 }, selected: true },
      { box: { left: 260, right: 300, top: 100, bottom: 124 }, selected: false },
    ]);
    expect(rows).toHaveLength(1);
    expect(rows[0]).toMatchObject({ top: 100, bottom: 124 });
  });

  it("splits rows whose glyph boxes overlap under a tight line height", () => {
    const rows = groupRows([
      { box: { left: 106, right: 400, top: 100, bottom: 126 }, selected: true },
      { box: { left: 106, right: 300, top: 122, bottom: 148 }, selected: true },
    ]);
    expect(rows).toHaveLength(2);
  });
});

describe("bandRows", () => {
  it("meets halfway between two rows' glyphs and reaches the line's edges", () => {
    expect(
      bandRows(
        [
          { top: 103, bottom: 121 },
          { top: 127, bottom: 145 },
        ],
        { top: 100, bottom: 148 },
      ),
    ).toEqual([
      { top: 100, bottom: 124 },
      { top: 124, bottom: 148 },
    ]);
  });
});

describe("mergeSpans", () => {
  it("joins boxes across a gap that holds no unselected character", () => {
    expect(mergeSpans([glyphs(106, 180), glyphs(184, 240)], [])).toEqual([{ left: 106, right: 240 }]);
  });

  it("keeps a gap that holds an unselected character open", () => {
    expect(mergeSpans([glyphs(106, 180), glyphs(240, 300)], [glyphs(190, 230)])).toEqual([
      { left: 106, right: 180 },
      { left: 240, right: 300 },
    ]);
  });
});

describe("clipToColumn", () => {
  it("drops a box that lies wholly in a gutter", () => {
    expect(clipToColumn([{ left: 40, right: 90, top: 0, bottom: 10 }], COLUMN)).toEqual([]);
  });
});

describe("replaceBoxes", () => {
  const container = { left: 106, right: 498, top: 130, bottom: 270 };
  const image = { left: 106, right: 234, top: 138, bottom: 266 };

  it("swaps a line-wide widget container for its narrower content", () => {
    expect(replaceBoxes([glyphs(106, 200), container], [container], [image])).toEqual([glyphs(106, 200), image]);
  });

  it("keeps a widget whose content is as wide as its box", () => {
    const chip = glyphs(200, 260);
    expect(replaceBoxes([chip], [chip], [glyphs(200.5, 259.5)])).toEqual([chip]);
  });

  it("leaves the boxes alone when the widget's box is not among them", () => {
    expect(replaceBoxes([glyphs(106, 200)], [container], [image])).toEqual([glyphs(106, 200)]);
  });
});
