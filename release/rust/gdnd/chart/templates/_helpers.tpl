{{/*
Expand the name of the chart.
*/}}
{{- define "gdnd.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
*/}}
{{- define "gdnd.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Create chart name and version as used by the chart label.
*/}}
{{- define "gdnd.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Common labels
*/}}
{{- define "gdnd.labels" -}}
helm.sh/chart: {{ include "gdnd.chart" . }}
{{ include "gdnd.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/*
Selector labels
*/}}
{{- define "gdnd.selectorLabels" -}}
app.kubernetes.io/name: {{ include "gdnd.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
Create the name of the service account to use
*/}}
{{- define "gdnd.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "gdnd.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}

{{/*
Create the config file content
*/}}
{{- define "gdnd.config" -}}
device_type: {{ .Values.config.deviceType }}
l1_interval: {{ .Values.config.l1Interval }}
l2_interval: {{ .Values.config.l2Interval }}
l3_interval: {{ .Values.config.l3Interval }}
l3_enabled: {{ .Values.config.l3Enabled }}
gpu_check_path: {{ .Values.config.gpuCheckPath }}

health:
  failure_threshold: {{ .Values.config.health.failureThreshold }}
  fatal_xids:
    {{- range .Values.config.health.fatalXids }}
    - {{ . }}
    {{- end }}
  temperature_threshold: {{ .Values.config.health.temperatureThreshold }}
  active_check_timeout: {{ .Values.config.health.activeCheckTimeout }}

isolation:
  cordon: {{ .Values.config.isolation.cordon }}
  evict_pods: {{ .Values.config.isolation.evictPods }}
  taint_key: {{ .Values.config.isolation.taintKey }}
  taint_value: {{ .Values.config.isolation.taintValue }}
  taint_effect: {{ .Values.config.isolation.taintEffect }}

metrics:
  enabled: {{ .Values.metrics.enabled }}
  port: {{ .Values.metrics.port }}
  path: {{ .Values.metrics.path }}

dry_run: {{ .Values.config.dryRun }}
{{- end }}
