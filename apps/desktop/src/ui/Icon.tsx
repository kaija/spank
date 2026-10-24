import type { CSSProperties } from "react";
import { ICON_PATHS, type IconName } from "./iconPaths";

type Props = {
  name: IconName;
  size?: number;
  strokeWidth?: number;
  style?: CSSProperties;
  /** Accessible label; omit for decorative icons. */
  label?: string;
};

/** Line icon from the mockup set (matches `window.spankIcon` in mockup/spank-icons.js). */
export function Icon({ name, size = 16, strokeWidth = 1.75, style, label }: Props) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill={name === "play" ? "currentColor" : "none"}
      stroke="currentColor"
      strokeWidth={strokeWidth}
      strokeLinecap="round"
      strokeLinejoin="round"
      role={label ? "img" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
      style={{ display: "block", flex: "none", ...style }}
      // Static, trusted path strings from ICON_PATHS only.
      dangerouslySetInnerHTML={{ __html: ICON_PATHS[name] }}
    />
  );
}
