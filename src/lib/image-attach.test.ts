import { describe, expect, it } from "vitest";
import { imageFilesOnly, splitDataUrl } from "@/lib/image-attach";

describe("splitDataUrl", () => {
  it("splits a png data URL into media type and base64", () => {
    const { mediaType, data } = splitDataUrl("data:image/png;base64,AAAABBBB");
    expect(mediaType).toBe("image/png");
    expect(data).toBe("AAAABBBB");
  });

  it("handles jpeg", () => {
    expect(splitDataUrl("data:image/jpeg;base64,ZZZ").mediaType).toBe("image/jpeg");
  });

  it("throws on a plain URL", () => {
    expect(() => splitDataUrl("https://example.com/x.png")).toThrow();
  });

  it("throws on a non-base64 data URL", () => {
    expect(() => splitDataUrl("data:image/png,rawtext")).toThrow();
  });
});

describe("imageFilesOnly", () => {
  const png = new File(["x"], "a.png", { type: "image/png" });
  const jpg = new File(["y"], "b.jpg", { type: "image/jpeg" });
  const txt = new File(["z"], "c.txt", { type: "text/plain" });

  it("keeps only image files, preserving order", () => {
    expect(imageFilesOnly([png, txt, jpg]).map((f) => f.name)).toEqual(["a.png", "b.jpg"]);
  });

  it("returns empty when there are no images", () => {
    expect(imageFilesOnly([txt])).toEqual([]);
  });
});
