/** WCAG relative luminance for opaque area colors and generated sidebar tokens. */
export function areaDotNeedsRing(color: string | undefined, background: string): boolean {
  const luminance = (hex: string | undefined): number | null => {
    if (!hex || !/^#[\da-f]{6}$/i.test(hex.trim())) return null;
    const channels = [1, 3, 5].map(i => {
      const value = parseInt(hex.trim().slice(i, i + 2), 16) / 255;
      return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
    });
    return channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722;
  };
  const fill = luminance(color), surface = luminance(background);
  return fill !== null && surface !== null && (Math.max(fill, surface) + 0.05) / (Math.min(fill, surface) + 0.05) < 1.5;
}
