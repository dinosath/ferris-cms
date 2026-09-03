{{/* Expand the full name of the chart/release. */}}
{{- define "ferriscms.fullname" -}}
{{- if .Values.fullnameOverride -}}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- $name := default .Chart.Name .Values.nameOverride -}}
{{- if contains $name .Release.Name -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}
{{- end -}}

{{/* Chart name. */}}
{{- define "ferriscms.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/* Common labels. */}}
{{- define "ferriscms.labels" -}}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 }}
app.kubernetes.io/name: {{ include "ferriscms.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
app.kubernetes.io/part-of: ferriscms
{{- end -}}

{{/* Selector labels. */}}
{{- define "ferriscms.selectorLabels" -}}
app.kubernetes.io/name: {{ include "ferriscms.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end -}}

{{/* Image reference with tag fallback to appVersion. */}}
{{- define "ferriscms.image" -}}
{{- $tag := .Values.image.tag | default .Chart.AppVersion -}}
{{- printf "%s:%s" .Values.image.repository $tag -}}
{{- end -}}

{{/* Resolved admin username (default "admin"). */}}
{{- define "ferriscms.adminUsername" -}}
{{- .Values.admin.username | default "admin" -}}
{{- end -}}

{{/* Resolved admin email: explicit value, else "<username>@ferriscms.local". */}}
{{- define "ferriscms.adminEmail" -}}
{{- $username := include "ferriscms.adminUsername" . -}}
{{- .Values.admin.email | default (printf "%s@ferriscms.local" $username) -}}
{{- end -}}

{{/*
Resolved admin password.

Uses .Values.admin.password when provided. Otherwise, on an upgrade/render of an
existing release, it reuses the password already stored in the chart's ConfigMap
so credentials stay stable across `helm upgrade`; on a fresh install it generates
a strong random password with `randAlphaNum`.
*/}}
{{- define "ferriscms.adminPassword" -}}
{{- if .Values.admin.password -}}
{{- .Values.admin.password -}}
{{- else -}}
{{- $cmName := printf "%s-admin" (include "ferriscms.fullname" .) -}}
{{- $existing := (lookup "v1" "ConfigMap" .Release.Namespace $cmName).data -}}
{{- if index ($existing | default dict) "ADMIN_PASSWORD" -}}
{{- index $existing "ADMIN_PASSWORD" -}}
{{- else -}}
{{- randAlphaNum 24 -}}
{{- end -}}
{{- end -}}
{{- end -}}

