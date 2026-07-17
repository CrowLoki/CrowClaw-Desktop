import crowClawMark from "../assets/branding/crowclaw-mark.webp";

type BrandMarkProps = {
  compact?: boolean;
};

export function BrandMark({ compact = false }: BrandMarkProps) {
  return (
    <div className={compact ? "brand-mark brand-mark--compact" : "brand-mark"} aria-label="CrowClaw">
      <span className="brand-mark__glyph" aria-hidden="true">
        <img src={crowClawMark} alt="" />
      </span>
      {!compact && (
        <span className="brand-mark__wordmark">
          CROW<span>CLAW</span>
        </span>
      )}
    </div>
  );
}

