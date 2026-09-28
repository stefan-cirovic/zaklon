import type { Key } from "../i18n";
import { helpHref, type HelpTopicId } from "../help";
import { Icon } from "./Icon";

/**
 * "How it works" at the top of a screen: opens the screen's page in Help (or
 * one section of it). On a phone only the "?" shows; the words stay for
 * screen readers.
 */
export default function HelpLink({ t, topic, section, className }: { t: (k: Key) => string; topic: HelpTopicId; section?: string; className?: string }) {
  const label = t("helpHowItWorks");
  return (
    <a className={"help-link" + (className ? ` ${className}` : "")} href={helpHref(topic, section)} title={label}>
      <Icon name="help" size={18} />
      <span className="help-link-text">{label}</span>
    </a>
  );
}
