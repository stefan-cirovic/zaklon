export default function Placeholder({ title, text }: { title: string; text: string }) {
  return (
    <div>
      <h1>{title}</h1>
      <p className="muted">{text}</p>
    </div>
  );
}
