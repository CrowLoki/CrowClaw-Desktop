type BrandMarkProps = {
  compact?: boolean;
};

export function BrandMark({ compact = false }: BrandMarkProps) {
  return (
    <div className={compact ? "brand-mark brand-mark--compact" : "brand-mark"} aria-label="CrowClaw">
      <span className="brand-mark__glyph" aria-hidden="true">
        <span />
        <span />
        <span />
      </span>
      {!compact && (
        <span className="brand-mark__wordmark">
          Crow<span>Claw</span>
        </span>
      )}
    </div>
  );
}

