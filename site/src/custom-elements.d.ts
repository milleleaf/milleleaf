import type { DetailedHTMLProps, HTMLAttributes } from 'react'
declare module 'react' {
  namespace JSX {
    interface IntrinsicElements {
      'ml-verify': DetailedHTMLProps<HTMLAttributes<HTMLElement>, HTMLElement>
      'ml-theme': DetailedHTMLProps<HTMLAttributes<HTMLElement>, HTMLElement>
      'ml-revisions': DetailedHTMLProps<HTMLAttributes<HTMLElement>, HTMLElement>
    }
  }
}
