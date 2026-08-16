ask: Align Ferris CMS Admin UI with Strapi 5 You are working on: - Repository: "https://github.com/dinosath/ferris-cms" - Main branch: "main" - Stack: Rust + Dioxus - Target UI: Ferris CMS admin panel - 
Primary reference implementation: official Strapi 5 repository - Reference repository: "https://github.com/strapi/strapi" - Reference design system: "https://github.com/strapi/design-system" Your goal is 
to make Ferris CMS's Content Manager and Content-Type Builder visually and interactionally feel like a polished Strapi 5 admin panel. This is an implementation task, not a conceptual redesign. Do not 
invent a new visual language. Study the official Strapi implementation and reproduce its UX patterns, information hierarchy, spacing, controls, buttons, forms, tables, navigation, dialogs, drawers, empty 
states, and responsive behavior within Ferris CMS. However, while Strapi parity is the visual and UX target, all implementation must remain idiomatic to Rust and Dioxus best practices. Never compromise 
Ferris CMS architecture, safety, or component design principles for the sake of visual matching. 1. First: inspect both repositories Before changing code: 1. Thoroughly inspect "dinosath/ferris-cms". 2. 
Identify:
   - application layout - admin layout - sidebar/navigation - Content Manager screens - Content-Type Builder screens - reusable UI components - buttons - inputs - selects - tables - dialogs/modals - 
   drawers - forms - cards - notifications/toasts - breadcrumbs - tabs - dropdown menus - icons - typography - spacing utilities - responsive behavior
3. Locate the existing Content Manager and Content-Type Builder implementation. 4. Do not replace working functionality merely to achieve visual similarity. Then inspect the corresponding Strapi 5 
implementation in: "https://github.com/strapi/strapi" Pay particular attention to the official Content Manager and Content-Type Builder implementations. Also inspect: 
"https://github.com/strapi/design-system" Use the Strapi repository as the source of truth for behavior and UI structure. Do not copy Strapi code directly if doing so would conflict with Ferris CMS's 
Rust/Dioxus architecture or licensing requirements. Reimplement the relevant behavior and visual patterns idiomatically in Rust/Dioxus, following safe ownership, component composition, and reactive state 
patterns. 2. Establish Strapi as the visual/UX reference Treat Strapi 5 as the design specification. The following areas must be aligned: Global admin shell Match Strapi's: - sidebar width - sidebar 
hierarchy - navigation item height - selected/hover states - icon placement - typography - page background - content area width - header height - breadcrumbs - page titles - action areas - spacing between 
major sections - responsive behavior The UI should immediately feel like the same class of application as Strapi. Do not make Ferris CMS look like a generic dashboard template. At the same time, ensure 
layout composition follows Dioxus best practices (component reuse, props-driven layout, minimal duplication, and clean separation of concerns). 3. Design-system alignment Create or improve a reusable 
Ferris CMS design system rather than styling every page independently. Establish consistent primitives for: - Button - IconButton - Link - Input - Textarea - Select - Combobox - Checkbox - Radio - Switch 
- Date/time input - Field - Field label - Field hint - Field error - Form section - Card - Table - Table row - Badge - Status indicator - Tabs - Breadcrumbs - Dropdown - Popover - Modal/Dialog - Drawer - 
Toast/notification - Tooltip - Empty state - Loading state - Skeleton - Pagination - Search - Filter controls Every screen should use these primitives. Avoid screen-specific CSS whenever a reusable 
component can express the same UI. Ensure components follow Rust/Dioxus idioms: - composable props - minimal internal mutation - clear state ownership boundaries - reusable view functions - no duplicated 
rendering logic 4. Buttons Buttons are especially important. Align Ferris CMS buttons with Strapi's visual hierarchy. Define clear variants such as: - Primary - Secondary - Tertiary/Ghost - Danger - 
Disabled - Icon-only Standardize: - height - padding - border radius - font weight - font size - icon size - icon-to-text gap - hover state - active state - focus state - disabled state - loading state Do 
not use arbitrary button sizes throughout the application. Actions should have the same visual importance as their Strapi equivalents. Examples: - Create - Add field - Save - Publish - Unpublish - Delete 
- Cancel - Configure - Edit - Back - Continue All button components should be implemented as reusable Dioxus components with typed props, not ad-hoc markup. 5. Content Manager Make the Ferris CMS Content 
Manager behave and look like Strapi's Content Manager. It should support the same high-level information architecture. Content-type navigation Provide a clear way to navigate between: - Collection Types - 
Single Types The navigation should visually distinguish the currently selected content type. Content list Align the content list with Strapi. Include appropriate: - page header - content type title - 
Create new entry action - search - filters - sorting - table - selection - row actions - pagination - empty states - loading states Table columns must have consistent: - alignment - typography - row 
height - spacing - hover behavior - selection behavior - action placement Do not make the table look like a generic HTML table. Ensure table implementation is a shared reusable Dioxus component, not 
duplicated per screen. Content entry editor The entry editing screen should follow Strapi's hierarchy: - page header - breadcrumbs/back navigation - title - primary actions - content fields - field 
groups/sections - sidebar metadata/actions where appropriate - save/publish controls Fields should be visually consistent and easy to scan. Required/optional status, descriptions, validation errors, and 
hints should follow one consistent pattern. Draft / publish Treat content lifecycle actions as first-class UI. Visually distinguish: - Draft - Published - Modified - Unpublished Use consistent status 
badges and action hierarchy. The primary action should be obvious. 6. Content-Type Builder This is one of the highest-priority screens. Make it visually and interactionally resemble Strapi's Content-Type 
Builder. The builder should clearly distinguish: - Collection Types - Single Types - Components Content-type list Use a Strapi-like navigation structure. Each content type should expose: - display name - 
API identifier - type - actions The selected content type should be clearly highlighted. Content-type editor The editor should contain a clear hierarchy: 1. Content type header 2. Content type information 
3. Fields 4. Field configuration 5. Relations 6. Advanced/settings configuration 7. Save action The user should immediately understand: «"I am defining the schema of this content type."» Fields Fields 
should be displayed as structured rows/cards rather than arbitrary form elements. Each field should clearly show: - field name - field type - important configuration - required status - actions Add field 
The Add Field interaction should follow Strapi's pattern. Provide a clear field-type selection experience. Organize field types logically. Field configuration Field configuration must have a strong 
hierarchy: - Basic settings - Validation - Default value - Advanced settings 7. Modals, drawers and overlays Study how Strapi uses: - modal dialogs - drawers - popovers - confirmation dialogs Reproduce 
the same UX principles. Ensure: - correct focus handling - predictable action placement - keyboard navigation support - proper overlay behavior Implement these using safe Dioxus state patterns and 
reusable overlay components, not ad-hoc DOM logic. 8. Typography Create a coherent typography scale. Standardize: - page title - section title - subsection title - body - label - hint - caption - table 
text - button text - error text Avoid arbitrary font sizes. Typography should communicate hierarchy before the user reads the content. 9. Spacing Perform a complete spacing audit. Establish a consistent 
spacing scale and enforce it across all components. 10. Icons Use a consistent icon system. Ensure: - consistent size - consistent alignment - semantic meaning 11. Forms All forms should share the same 
structure: Label → Input → Hint → Validation state 12. Tables Create a reusable CMS table component in Dioxus. Must support: - sorting - selection - pagination - empty/loading states 13. Responsive design 
Follow Strapi behavior but ensure: - layout remains usable - components reflow correctly - no broken hierarchy 14. Accessibility Use proper semantic structure via Dioxus. Ensure: - keyboard navigation - 
focus visibility - ARIA correctness - screen reader support 15. Dioxus/Rust architecture This is critical: - Keep all UI idiomatic to Dioxus - Use component composition, not duplication - Prefer pure view 
functions - Keep state localized unless shared - Avoid over-engineered abstractions - Do not introduce non-Rust frontend paradigms Strapi parity must never override: - Rust safety - ownership clarity - 
component reusability - maintainability 16. Do not break backend functionality Preserve: - API contracts - authentication - CRUD logic - schema system - routing - permissions 17. Compare screen-by-screen 
Match Strapi behavior for: - Content Manager flows - Content-Type Builder flows - modals, drawers, tables, forms 18. Use official Strapi implementation as source of truth Prefer: - "strapi/strapi" - 
"strapi/design-system" 19. Important visual principle The final result should pass this test: «It feels like Strapi, but is clearly a Rust/Dioxus-native application under the hood.» 20. Implementation 
order 1. Audit 2. Design primitives 3. Admin shell 4. Content Manager 5. Content-Type Builder 6. Interaction polish 7. Responsive/accessibility audit 8. Cleanup 21. Testing Run: - "cargo fmt" - "cargo 
check" - existing tests 22. Final quality bar Ensure consistency across: - spacing - typography - buttons - tables - dialogs - navigation 23. Important constraint Ferris CMS is not a Strapi clone 
internally. Only the admin UX and visual language should align with Strapi. The final implementation must be: Strapi-like UX + Ferris CMS architecture + idiomatic Rust/Dioxus design system.
Start by inspecting the existing repository and the official Strapi implementation before writing code.
