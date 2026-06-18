# 06 — Redaction Policy

> This document defines what must never appear in the public portfolio and
> what can safely be included. It serves as a checklist before publishing
> any profile material.

---

## Must NEVER appear

### Client & company identity

- [ ] Client names (Getronics, specific banks, financial institutions)
- [ ] Internal project code names or system names
- [ ] Department names, team names, reporting structures
- [ ] Colleague full names (GitHub usernames from the public repo are OK
      since they're already public)

### Credentials & secrets

- [ ] API keys, tokens, passwords, connection strings
- [ ] Secrets vault contents or encryption keys
- [ ] `.env` files or environment variable values
- [ ] Private key paths, certificate fingerprints

### Infrastructure details

- [ ] Internal URLs, IP addresses, hostnames, domains
- [ ] Network topology, firewall rules, subnet ranges
- [ ] Server specifications, cluster sizes, instance counts
- [ ] Database connection details (hosts, ports, SIDs)

### Metrics & business data

- [ ] Exact transaction volumes or throughput numbers
- [ ] Revenue figures, cost data, budget numbers
- [ ] SLA percentages, incident counts, uptime guarantees
- [ ] User counts, customer numbers, market share

### Architecture & implementation

- [ ] Internal architecture diagrams or topology maps
- [ ] Proprietary algorithms or business logic
- [ ] Database schemas for internal systems
- [ ] Source code from closed-source repositories
- [ ] Internal API specifications or protocol documentation

### Screenshots & media

- [ ] Screenshots of internal dashboards, tools, or applications
- [ ] Photos of offices, desks, or colleagues
- [ ] Internal presentation slides or documents
- [ ] Recordings of internal meetings

### Regulatory & compliance

- [ ] Specific regulatory requirements or compliance frameworks
- [ ] Audit findings or security assessment results
- [ ] Business continuity or disaster recovery plans

---

## Can safely appear (if sanitized)

### General technology landscape

- [x] Technology names: DB2, Kubernetes, OpenShift, CI/CD tools
- [x] Architecture patterns: batch processing, API integration,
      containerized workloads, monitoring dashboards
- [x] Role descriptions: "built dashboards," "automated pipelines,"
      "integrated heterogeneous systems"

### Complexity descriptions (without specifics)

- [x] "Large-scale batch processing pipelines"
- [x] "Multi-system operational monitoring"
- [x] "Heterogeneous system integration spanning mainframe and cloud"
- [x] "Strict reliability requirements"
- [x] "Regulated environment with audit trail requirements"

### Personal contributions

- [x] "I designed and built..."
- [x] "I automated..."
- [x] "I integrated..."
- [x] Specific technologies I personally used

### Patterns & lessons learned

- [x] "This experience taught me to design for graceful degradation"
- [x] "Audit trails became a first-class concern"
- [x] "Security by design, not retrofit"

---

## Examples: Unsafe vs. Safe wording

### Example 1: Project identity

| ❌ Unsafe | ✅ Safe |
|-----------|---------|
| "At Getronics, I built the Cielo payment dashboard for Itaú..." | "In a banking enterprise environment, I built operational dashboards for payment processing systems..." |

### Example 2: Metrics

| ❌ Unsafe | ✅ Safe |
|-----------|---------|
| "The system processed 2.3M transactions/day with 99.97% uptime..." | "The system handled high-volume financial transactions with strict reliability requirements..." |

### Example 3: Architecture

| ❌ Unsafe | ✅ Safe |
|-----------|---------|
| "The Kubernetes cluster had 47 nodes across 3 AZs, with Istio service mesh connecting to DB2 host db2prod01.internal..." | "The infrastructure spanned Kubernetes/OpenShift workloads and DB2 databases, requiring resilient integration layers..." |

### Example 4: Team

| ❌ Unsafe | ✅ Safe |
|-----------|---------|
| "I managed a team of 12 engineers across São Paulo and Bangalore..." | "I worked within operations and engineering teams..." |

### Example 5: Tools

| ❌ Unsafe | ✅ Safe |
|-----------|---------|
| "We used Grafana with Prometheus scraping 300+ targets, Alertmanager routing to PagerDuty..." | "We used industry-standard observability stacks including metrics collection and alerting..." |

---

## Review checklist (use before publishing)

1. [ ] Read every mention of a company or client — is it necessary?
2. [ ] Search for IP addresses, hostnames, URLs — any still present?
3. [ ] Check all numbers — are they exact or generalized?
4. [ ] Verify no internal project names slipped through
5. [ ] Confirm all screenshots are from public/open-source work
6. [ ] Re-read from the perspective of a former employer's security team
7. [ ] If unsure about a specific phrase, default to removing it
