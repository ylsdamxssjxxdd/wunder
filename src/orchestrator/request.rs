use super::*;

impl Orchestrator {
    fn prepare_request(
        &self,
        request: WunderRequest,
    ) -> Result<PreparedRequest, OrchestratorError> {
        let user_id = request.user_id.trim().to_string();
        if user_id.is_empty() {
            return Err(OrchestratorError::invalid_request(i18n::t(
                "error.user_id_required",
            )));
        }
        if let Err(err) = self.workspace.ensure_user_root(&user_id) {
            return Err(OrchestratorError::internal(format!(
                "failed to prepare workspace: {err}"
            )));
        }
        self.workspace.touch_user_session(&user_id);
        let question = request.question.trim().to_string();
        if question.is_empty() {
            return Err(OrchestratorError::invalid_request(i18n::t(
                "error.question_required",
            )));
        }
        let session_id = request
            .session_id
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
        let tool_names = if request.tool_names.is_empty() {
            None
        } else {
            Some(request.tool_names.clone())
        };
        let language = request
            .language
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(i18n::get_default_language);
        let attachments = request
            .attachments
            .clone()
            .filter(|items| !items.is_empty());
        Ok(PreparedRequest {
            user_id,
            question,
            session_id,
            tool_names,
            skip_tool_calls: request.skip_tool_calls,
            model_name: request.model_name.clone(),
            config_overrides: request.config_overrides.clone(),
            stream: request.stream,
            debug_payload: request.debug_payload,
            attachments,
            language,
        })
    }

    pub async fn run(&self, request: WunderRequest) -> Result<WunderResponse> {
        let prepared = self.prepare_request(request)?;
        let language = prepared.language.clone();
        let emitter = EventEmitter::new(
            prepared.session_id.clone(),
            prepared.user_id.clone(),
            None,
            None,
            self.monitor.clone(),
        );
        let response = i18n::with_language(language, async {
            self.execute_request(prepared, emitter).await
        })
        .await?;
        Ok(response)
    }

    pub async fn stream(
        &self,
        request: WunderRequest,
    ) -> Result<impl Stream<Item = Result<StreamEvent, std::convert::Infallible>>> {
        let prepared = self.prepare_request(request)?;
        let language = prepared.language.clone();
        let (queue_tx, queue_rx) = mpsc::channel::<StreamSignal>(STREAM_EVENT_QUEUE_SIZE);
        let (event_tx, event_rx) = mpsc::channel::<StreamEvent>(STREAM_EVENT_QUEUE_SIZE);
        let emitter = EventEmitter::new(
            prepared.session_id.clone(),
            prepared.user_id.clone(),
            Some(queue_tx),
            Some(self.storage.clone()),
            self.monitor.clone(),
        );
        let runner = {
            let orchestrator = self.clone();
            let emitter = emitter.clone();
            let prepared = prepared.clone();
            let language = language.clone();
            tokio::spawn(async move {
                let result = i18n::with_language(language, async {
                    orchestrator.execute_request(prepared, emitter).await
                })
                .await;
                if let Err(err) = result {
                    warn!("流式请求执行失败: {}", err);
                }
            })
        };
        self.spawn_stream_pump(
            prepared.session_id.clone(),
            queue_rx,
            event_tx,
            emitter,
            runner,
        );
        let stream = tokio_stream::wrappers::ReceiverStream::new(event_rx)
            .map(|event| Ok::<_, std::convert::Infallible>(event));
        Ok(stream)
    }

    pub async fn build_system_prompt(
        &self,
        config: &Config,
        tool_names: &[String],
        skills: &SkillRegistry,
        user_tool_bindings: Option<&UserToolBindings>,
        user_id: &str,
        config_overrides: Option<&Value>,
    ) -> String {
        let allowed_tool_names =
            self.resolve_allowed_tool_names(config, tool_names, skills, user_tool_bindings);
        let tool_call_mode = self.resolve_tool_call_mode(config, None);
        let prompt = self
            .build_system_prompt_with_allowed(
                config,
                config_overrides,
                &allowed_tool_names,
                tool_call_mode,
                skills,
                user_tool_bindings,
                user_id,
            )
            .await;
        self.append_memory_prompt(user_id, prompt).await
    }

}
