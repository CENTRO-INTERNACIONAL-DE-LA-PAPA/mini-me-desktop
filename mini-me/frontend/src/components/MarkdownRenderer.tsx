import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

// Default export so MarkdownContent can React.lazy() this module, keeping
// react-markdown + remark-gfm out of the initial bundle until the first
// message actually needs to render.
export default function MarkdownRenderer({ children }: { children: string }) {
  return <ReactMarkdown remarkPlugins={[remarkGfm]}>{children}</ReactMarkdown>;
}
