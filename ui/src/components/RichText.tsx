import { Fragment } from "react";
import { parseInline } from "../help";

/**
 * A text of the app with the marks the help uses: **bold** and [a link to a
 * screen](#library). Only addresses inside the app are links.
 */
export default function RichText({ text }: { text: string }) {
  return (
    <>
      {parseInline(text).map((x, i) =>
        "link" in x ? (
          <a key={i} href={x.href}>
            {x.link}
          </a>
        ) : "bold" in x ? (
          <strong key={i}>{x.bold}</strong>
        ) : (
          <Fragment key={i}>{x.text}</Fragment>
        ),
      )}
    </>
  );
}
