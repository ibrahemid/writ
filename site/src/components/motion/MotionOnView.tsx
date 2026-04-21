import { motion, useReducedMotion } from 'framer-motion';
import type { CSSProperties, ReactNode } from 'react';

interface MotionOnViewProps {
  children: ReactNode;
  delay?: number;
  y?: number;
  duration?: number;
  className?: string;
  style?: CSSProperties;
  as?: 'div' | 'section' | 'article' | 'li' | 'header';
  once?: boolean;
}

export default function MotionOnView({
  children,
  delay = 0,
  y = 14,
  duration = 0.45,
  className,
  style,
  as = 'div',
  once = true,
}: MotionOnViewProps) {
  const reduce = useReducedMotion();
  const Component = motion[as];

  if (reduce) {
    return (
      <Component className={className} style={style}>
        {children}
      </Component>
    );
  }

  return (
    <Component
      className={className}
      style={style}
      initial={{ opacity: 0, y }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once, margin: '0px 0px -10% 0px' }}
      transition={{ duration, delay, ease: [0.33, 1, 0.68, 1] }}
    >
      {children}
    </Component>
  );
}
