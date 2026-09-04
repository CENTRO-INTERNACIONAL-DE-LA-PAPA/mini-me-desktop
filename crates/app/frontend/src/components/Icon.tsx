import { hex } from "../theme/theme";

export type IconSize = "extraSmall" | "small" | "medium" | "large";

const SIZE_PX: Record<IconSize, number> = {
  extraSmall: 12,
  small: 18,
  medium: 20,
  large: 22,
};

export function Icon({
  path,
  size = "small",
  colour,
  className,
}: {
  path: string;
  size?: IconSize;
  colour: number;
  className?: string;
}) {
  const px = SIZE_PX[size];
  return (
    <span
      className={className}
      style={{
        display: "inline-block",
        flex: "none",
        width: px,
        height: px,
        backgroundColor: hex(colour),
        WebkitMaskImage: `url(/${path})`,
        maskImage: `url(/${path})`,
        WebkitMaskSize: "contain",
        maskSize: "contain",
        WebkitMaskRepeat: "no-repeat",
        maskRepeat: "no-repeat",
        WebkitMaskPosition: "center",
        maskPosition: "center",
      }}
    />
  );
}
