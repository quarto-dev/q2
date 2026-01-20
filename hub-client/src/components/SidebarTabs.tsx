/**
 * Sidebar Sections Component
 *
 * VS Code-style collapsible sections that stack vertically.
 * Each section can be independently expanded/collapsed.
 */

import { useState, type ReactNode } from 'react';
import './SidebarTabs.css';

export type SectionId = 'files' | 'outline' | 'project' | 'status' | 'settings' | 'about';

interface Section {
  id: SectionId;
  label: string;
  defaultExpanded?: boolean;
}

const SECTIONS: Section[] = [
  { id: 'files', label: 'FILES', defaultExpanded: true },
  { id: 'outline', label: 'OUTLINE', defaultExpanded: true },
  { id: 'project', label: 'PROJECT', defaultExpanded: false },
  { id: 'status', label: 'STATUS', defaultExpanded: false },
  { id: 'settings', label: 'SETTINGS', defaultExpanded: false },
  { id: 'about', label: 'ABOUT', defaultExpanded: false },
];

interface SidebarTabsProps {
  children: (sectionId: SectionId) => ReactNode;
}

export default function SidebarTabs({ children }: SidebarTabsProps) {
  const [expandedSections, setExpandedSections] = useState<Set<SectionId>>(() => {
    const initial = new Set<SectionId>();
    for (const section of SECTIONS) {
      if (section.defaultExpanded) {
        initial.add(section.id);
      }
    }
    return initial;
  });

  const toggleSection = (sectionId: SectionId) => {
    setExpandedSections((prev) => {
      const next = new Set(prev);
      if (next.has(sectionId)) {
        next.delete(sectionId);
      } else {
        next.add(sectionId);
      }
      return next;
    });
  };

  return (
    <div className="sidebar-sections">
      {SECTIONS.map((section) => {
        const isExpanded = expandedSections.has(section.id);
        return (
          <div
            key={section.id}
            className={`sidebar-section ${isExpanded ? 'expanded' : 'collapsed'}`}
          >
            <button
              className="section-header"
              onClick={() => toggleSection(section.id)}
              aria-expanded={isExpanded}
            >
              <span className="section-chevron">{isExpanded ? '▼' : '▶'}</span>
              <span className="section-label">{section.label}</span>
            </button>
            {isExpanded && (
              <div className="section-content">
                {children(section.id)}
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}
