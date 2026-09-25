import { useEffect, useState } from "react";
import QRCode from "qrcode";

export default function Qr({ value, size = 220 }: { value: string; size?: number }) {
  const [url, setUrl] = useState<string>("");
  useEffect(() => {
    QRCode.toDataURL(value, { width: size, margin: 0, color: { dark: "#0b0d10", light: "#ffffff" } })
      .then(setUrl)
      .catch(() => setUrl(""));
  }, [value, size]);
  return url ? (
    <span className="qr"><img src={url} width={size} height={size} alt="QR code" /></span>
  ) : null;
}
