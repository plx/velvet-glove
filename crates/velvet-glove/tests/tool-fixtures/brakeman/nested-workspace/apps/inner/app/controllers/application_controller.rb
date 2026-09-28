class ApplicationController < ActionController::Base
  def index
    render inline: params[:name]
  end
end
